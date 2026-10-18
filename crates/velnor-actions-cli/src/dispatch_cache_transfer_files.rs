use super::cleanup::remove_tree_at;
use super::dirs::{open_parent, same_object, same_version, stat_optional, validate_file_stat};
use super::{FILE_DOMAIN, MAX_FILE_BYTES};
use rustix::fs::{AtFlags, Mode, OFlags, fchmod, openat, statat};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

pub(super) fn copy_file_to_new(source: &Path, destination: &Path) -> Result<String, &'static str> {
    let result = copy_file_to_new_inner(source, destination);
    if result.is_err()
        && let Ok((parent, name)) = open_parent(destination)
        && let Ok(Some(stat)) = stat_optional(&parent, name.as_c_str())
    {
        remove_tree_at(&parent, name.as_c_str(), &stat)?;
    }
    result
}

pub(super) fn copy_file_to_new_inner(
    source: &Path,
    destination: &Path,
) -> Result<String, &'static str> {
    let (mut source_file, source_stat) = open_copy_source(source)?;
    let (destination_parent, destination_name) = open_parent(destination)?;
    let destination_fd = openat(
        &destination_parent,
        destination_name.as_c_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::from_raw_mode(source_stat.st_mode & 0o777),
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    let destination_copy =
        rustix::io::dup(&destination_fd).map_err(|_| "qualification_cache_payload_open_failed")?;
    let mut destination_file = fs::File::from(destination_copy);
    let expected = copy_file_bytes(&mut source_file, &mut destination_file, &source_stat)?;
    verify_copied_file(
        &destination_fd,
        &destination_parent,
        &destination_name,
        destination,
        &source_stat,
        &expected,
    )
}

fn open_copy_source(source: &Path) -> Result<(fs::File, rustix::fs::Stat), &'static str> {
    let (parent, name) = open_parent(source)?;
    let before = statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_open_failed")?;
    validate_file_stat(&before)?;
    let fd = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    if !same_version(
        &before,
        &rustix::fs::fstat(&fd).map_err(|_| "qualification_cache_payload_open_failed")?,
    ) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok((fs::File::from(fd), before))
}

fn copy_file_bytes(
    source: &mut fs::File,
    destination: &mut fs::File,
    source_stat: &rustix::fs::Stat,
) -> Result<String, &'static str> {
    let size =
        u64::try_from(source_stat.st_size).map_err(|_| "qualification_cache_payload_limit")?;
    if size > MAX_FILE_BYTES {
        return Err("qualification_cache_payload_limit");
    }
    let mut digest = blake3::Hasher::new();
    digest.update(FILE_DOMAIN);
    digest.update(&(u32::from(source_stat.st_mode) & 0o777).to_be_bytes());
    digest.update(&size.to_be_bytes());
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|_| "qualification_cache_payload_read_failed")?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .ok_or("qualification_cache_payload_limit")?;
        if copied > size || copied > MAX_FILE_BYTES {
            return Err("qualification_cache_payload_file_changed");
        }
        digest.update(&buffer[..count]);
        destination
            .write_all(&buffer[..count])
            .map_err(|_| "qualification_cache_payload_write_failed")?;
    }
    if copied != size
        || !same_version(
            source_stat,
            &rustix::fs::fstat(source).map_err(|_| "qualification_cache_payload_read_failed")?,
        )
    {
        return Err("qualification_cache_payload_file_changed");
    }
    destination
        .sync_all()
        .map_err(|_| "qualification_cache_payload_write_failed")?;
    Ok(format!("b3-{}", digest.finalize().to_hex()))
}

fn verify_copied_file(
    destination_fd: &rustix::fd::OwnedFd,
    destination_parent: &rustix::fd::OwnedFd,
    destination_name: &std::ffi::CString,
    destination: &Path,
    source_stat: &rustix::fs::Stat,
    expected: &str,
) -> Result<String, &'static str> {
    fchmod(
        destination_fd,
        Mode::from_raw_mode(source_stat.st_mode & 0o777),
    )
    .map_err(|_| "qualification_cache_payload_write_failed")?;
    let destination_stat = rustix::fs::fstat(destination_fd)
        .map_err(|_| "qualification_cache_payload_write_failed")?;
    validate_file_stat(&destination_stat)?;
    let observed = digest_file(destination)?.ok_or("qualification_cache_payload_copy_mismatch")?;
    let current = statat(
        destination_parent,
        destination_name.as_c_str(),
        AtFlags::SYMLINK_NOFOLLOW,
    )
    .map_err(|_| "qualification_cache_payload_copy_mismatch")?;
    if observed != expected || !same_object(&destination_stat, &current) {
        return Err("qualification_cache_payload_copy_mismatch");
    }
    Ok(expected.to_owned())
}

pub(super) fn digest_file(path: &Path) -> Result<Option<String>, &'static str> {
    let (parent, name) = open_parent(path)?;
    let before = statat(&parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "qualification_cache_payload_open_failed")?;
    validate_file_stat(&before)?;
    let fd = openat(
        &parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "qualification_cache_payload_open_failed")?;
    if !same_version(
        &before,
        &rustix::fs::fstat(&fd).map_err(|_| "qualification_cache_payload_open_failed")?,
    ) {
        return Err("qualification_cache_payload_file_changed");
    }
    let size = u64::try_from(before.st_size).map_err(|_| "qualification_cache_payload_limit")?;
    if size > MAX_FILE_BYTES {
        return Err("qualification_cache_payload_limit");
    }
    let mut file = fs::File::from(fd);
    let mut digest = blake3::Hasher::new();
    digest.update(FILE_DOMAIN);
    digest.update(&(u32::from(before.st_mode) & 0o777).to_be_bytes());
    digest.update(&size.to_be_bytes());
    let mut count = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "qualification_cache_payload_read_failed")?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or("qualification_cache_payload_limit")?;
        if count > size || count > MAX_FILE_BYTES {
            return Err("qualification_cache_payload_file_changed");
        }
        digest.update(&buffer[..read]);
    }
    let after = rustix::fs::fstat(&file).map_err(|_| "qualification_cache_payload_read_failed")?;
    if count != size || !same_version(&before, &after) {
        return Err("qualification_cache_payload_file_changed");
    }
    Ok(Some(format!("b3-{}", digest.finalize().to_hex())))
}
