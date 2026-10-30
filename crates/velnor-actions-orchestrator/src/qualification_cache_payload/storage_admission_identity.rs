//! Read the image identity from its controller-mounted read-only mount.

use super::RuntimeIdentityV1;
#[cfg(target_os = "linux")]
use rustix::fs::{AtFlags, FileType, Mode, OFlags, StatxFlags, open, openat, statat, statx};
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::os::fd::AsFd;

#[cfg(target_os = "linux")]
const IDENTITY_DIRECTORY: &str = "/.velnor-runtime-identity";
#[cfg(target_os = "linux")]
const IDENTITY_FILE: &str = "runner.json";
#[cfg(target_os = "linux")]
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
#[cfg(target_os = "linux")]
const ROOT_UID: u32 = 0;
#[cfg(target_os = "linux")]
const MAX_IDENTITY_BYTES: usize = 4 * 1024;
#[cfg(target_os = "linux")]
const MAX_MOUNTINFO_BYTES: usize = 1024 * 1024;

#[cfg(target_os = "linux")]
pub(super) fn read_runtime_identity() -> Result<Option<RuntimeIdentityV1>, &'static str> {
    let root = open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_storage_runtime_identity_open_failed")?;
    let directory = match openat(
        &root,
        ".velnor-runtime-identity",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(directory) => directory,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
        Err(_) => return Err("cache_storage_runtime_identity_mount_invalid"),
    };
    let root_stat =
        rustix::fs::fstat(&root).map_err(|_| "cache_storage_runtime_identity_stat_failed")?;
    if FileType::from_raw_mode(root_stat.st_mode) != FileType::Directory
        || root_stat.st_uid != ROOT_UID
        || u32::from(root_stat.st_mode) & 0o022 != 0
    {
        return Err("cache_storage_runtime_identity_parent_invalid");
    }
    let directory_stat =
        rustix::fs::fstat(&directory).map_err(|_| "cache_storage_runtime_identity_stat_failed")?;
    let directory_x = statx(
        &directory,
        "",
        AtFlags::EMPTY_PATH,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "cache_storage_runtime_identity_stat_failed")?;
    let mask = StatxFlags::from_bits_retain(directory_x.stx_mask);
    let device = u64::try_from(directory_stat.st_dev)
        .map_err(|_| "cache_storage_runtime_identity_mount_invalid")?;
    if !mask.contains(StatxFlags::BASIC_STATS | StatxFlags::MNT_ID)
        || FileType::from_raw_mode(directory_stat.st_mode) != FileType::Directory
        || directory_stat.st_uid != ROOT_UID
        || u32::from(directory_stat.st_mode) & 0o7777 != 0o555
        || directory_stat.st_nlink < 2
        || directory_x.stx_ino != directory_stat.st_ino
        || u64::from(directory_x.stx_dev_major) != u64::from(rustix::fs::major(device))
        || u64::from(directory_x.stx_dev_minor) != u64::from(rustix::fs::minor(device))
    {
        return Err("cache_storage_runtime_identity_mount_invalid");
    }
    verify_read_only_mount(
        directory_x.stx_mnt_id,
        directory_x.stx_dev_major,
        directory_x.stx_dev_minor,
    )?;
    let identity = read_identity_file(&directory)?;
    let opened_directory =
        rustix::fs::fstat(&directory).map_err(|_| "cache_storage_runtime_identity_stat_failed")?;
    let current = statat(&root, ".velnor-runtime-identity", AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_storage_runtime_identity_mount_changed")?;
    let current_x = statx(
        &directory,
        "",
        AtFlags::EMPTY_PATH,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|_| "cache_storage_runtime_identity_stat_failed")?;
    if !same_directory(&directory_stat, &opened_directory)
        || !same_directory(&directory_stat, &current)
        || current_x.stx_mnt_id != directory_x.stx_mnt_id
        || current_x.stx_ino != directory_x.stx_ino
        || current_x.stx_dev_major != directory_x.stx_dev_major
        || current_x.stx_dev_minor != directory_x.stx_dev_minor
    {
        return Err("cache_storage_runtime_identity_mount_changed");
    }
    verify_read_only_mount(
        current_x.stx_mnt_id,
        current_x.stx_dev_major,
        current_x.stx_dev_minor,
    )?;
    Ok(Some(identity))
}

#[cfg(target_os = "linux")]
fn read_identity_file(directory: &rustix::fd::OwnedFd) -> Result<RuntimeIdentityV1, &'static str> {
    let descriptor = openat(
        directory,
        IDENTITY_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| "cache_storage_runtime_identity_file_open_failed")?;
    let before = rustix::fs::fstat(&descriptor)
        .map_err(|_| "cache_storage_runtime_identity_file_stat_failed")?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile
        || before.st_uid != ROOT_UID
        || u32::from(before.st_mode) & 0o7777 != 0o444
        || before.st_nlink != 1
        || before.st_size <= 0
        || u64::try_from(before.st_size).unwrap_or(u64::MAX) > MAX_IDENTITY_BYTES as u64
    {
        return Err("cache_storage_runtime_identity_file_invalid");
    }
    let file = File::from(descriptor);
    let mut bytes = Vec::with_capacity(
        usize::try_from(before.st_size)
            .unwrap_or_default()
            .min(MAX_IDENTITY_BYTES),
    );
    (&file)
        .take(u64::try_from(MAX_IDENTITY_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|_| "cache_storage_runtime_identity_file_read_failed")?;
    let after = rustix::fs::fstat(file.as_fd())
        .map_err(|_| "cache_storage_runtime_identity_file_stat_failed")?;
    let current = statat(directory, IDENTITY_FILE, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_storage_runtime_identity_file_changed")?;
    if bytes.is_empty()
        || bytes.len() > MAX_IDENTITY_BYTES
        || !same_file(&before, &after)
        || !same_file(&after, &current)
    {
        return Err("cache_storage_runtime_identity_file_changed");
    }
    decode_identity(&bytes)
}

#[cfg(test)]
fn decode_identity(bytes: &[u8]) -> Result<RuntimeIdentityV1, &'static str> {
    let identity: RuntimeIdentityV1 =
        serde_json::from_slice(bytes).map_err(|_| "cache_storage_runtime_identity_invalid")?;
    if serde_json::to_vec(&identity).map_err(|_| "cache_storage_runtime_identity_invalid")? != bytes
    {
        return Err("cache_storage_runtime_identity_noncanonical");
    }
    super::validation::validate_runtime_identity(&identity)?;
    Ok(identity)
}

#[cfg(target_os = "linux")]
fn verify_read_only_mount(
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
) -> Result<(), &'static str> {
    let mut file = File::open(MOUNTINFO_PATH)
        .map_err(|_| "cache_storage_runtime_identity_mountinfo_read_failed")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(u64::try_from(MAX_MOUNTINFO_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .map_err(|_| "cache_storage_runtime_identity_mountinfo_read_failed")?;
    if bytes.len() > MAX_MOUNTINFO_BYTES {
        return Err("cache_storage_runtime_identity_mountinfo_too_large");
    }
    let mountinfo = std::str::from_utf8(&bytes)
        .map_err(|_| "cache_storage_runtime_identity_mountinfo_invalid")?;
    super::mountinfo::validate_read_only_mountinfo(
        mountinfo,
        IDENTITY_DIRECTORY,
        mount_id,
        device_major,
        device_minor,
    )
}

#[cfg(target_os = "linux")]
fn same_directory(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino && left.st_mode == right.st_mode
}

#[cfg(target_os = "linux")]
fn same_file(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_mode == right.st_mode
        && left.st_nlink == right.st_nlink
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
}

#[cfg(test)]
pub(super) fn test_identity() -> RuntimeIdentityV1 {
    RuntimeIdentityV1 {
        domain: "velnor.runtime-identity".to_owned(),
        schema_version: 1,
        runner_image_name: "velnor-runner".to_owned(),
        runner_source_revision: "1".repeat(40),
        runner_manifest_digest: format!("sha256:{}", "a".repeat(64)),
        runner_config_digest: format!("sha256:{}", "b".repeat(64)),
        platform: "linux/amd64".to_owned(),
    }
}

#[cfg(test)]
#[path = "storage_admission_identity_tests.rs"]
mod tests;
