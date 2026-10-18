use super::{StageError, stage::open_parent};
use rustix::fs::{AtFlags, FileType, OFlags, openat, statat};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

const STAGED_HELPER_PREFIX: &str = "velnor-actions-";
const ALIAS_NAMES: [&str; 2] = ["tar", "gtar"];
const MAX_HELPER_BYTES: u64 = 512 * 1024 * 1024;

/// Private operation tag for installing the verified tar aliases.
pub const CACHE_TAR_LAUNCHER_INSTALL_OP: &str = "install-cache-tar-aliases-v1";

/// Installs `tar` and `gtar` hard links beside the digest-verified staged CLI.
///
/// The function derives the executable path from `runner_temp` and `version`.
/// The current process must run the file at that path.
///
/// # Errors
///
/// If the helper, digest, permissions, or alias-path checks give unsatisfactory
/// results, the function gives an error. Existing aliases are never replaced.
pub fn install_cache_tar_launchers(
    runner_temp: &Path,
    version: &str,
    expected_sha256: &str,
) -> Result<PathBuf, StageError> {
    if !velnor_actions_contract::is_exact_semver(version) || !valid_sha256(expected_sha256) {
        return Err("cache_tar_launcher_identity_invalid");
    }
    let runner_temp = canonical_runner_temp(runner_temp)?;
    let alias_dir = runner_temp.join("velnor/bin");
    let executable = alias_dir.join(format!("{STAGED_HELPER_PREFIX}{version}"));
    let current = std::env::current_exe().map_err(|_| "cache_tar_launcher_executable_invalid")?;
    let current =
        std::fs::canonicalize(current).map_err(|_| "cache_tar_launcher_executable_invalid")?;
    if current != executable {
        return Err("cache_tar_launcher_executable_mismatch");
    }
    install_aliases(&executable, &alias_dir, expected_sha256)?;
    Ok(alias_dir)
}

fn install_aliases(
    executable: &Path,
    alias_dir: &Path,
    expected_sha256: &str,
) -> Result<(), StageError> {
    if !valid_sha256(expected_sha256) || executable.parent() != Some(alias_dir) {
        return Err("cache_tar_launcher_identity_invalid");
    }
    let (parent, source_name) = open_parent(executable)?;
    let mut source = open_source(&parent, &source_name)?;
    let source_stat =
        rustix::fs::fstat(source.as_fd()).map_err(|_| "cache_tar_launcher_stat_failed")?;
    verify_source(
        &mut source,
        &parent,
        &source_name,
        &source_stat,
        expected_sha256,
    )?;

    let mut installed = Vec::with_capacity(ALIAS_NAMES.len());
    for alias in ALIAS_NAMES {
        let alias_name = CString::new(alias).map_err(|_| "cache_tar_launcher_alias_invalid")?;
        if rustix::fs::linkat(
            &parent,
            source_name.as_c_str(),
            &parent,
            alias_name.as_c_str(),
            AtFlags::empty(),
        )
        .is_err()
        {
            rollback_aliases(&parent, &installed, &source_stat)?;
            return Err("cache_tar_launcher_collision_or_link_failed");
        }
        match statat(&parent, alias_name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Ok(alias_stat) if same_inode(&source_stat, &alias_stat) => {
                installed.push(alias_name);
            }
            _ => {
                rollback_aliases(&parent, &installed, &source_stat)?;
                unlink_if_same(&parent, &alias_name, &source_stat)?;
                return Err("cache_tar_launcher_link_mismatch");
            }
        }
    }
    let after = rustix::fs::fstat(source.as_fd()).map_err(|_| "cache_tar_launcher_stat_failed")?;
    if !same_identity_and_content(&source_stat, &after) {
        rollback_aliases(&parent, &installed, &source_stat)?;
        return Err("cache_tar_launcher_source_changed");
    }
    Ok(())
}

fn open_source(parent: &OwnedFd, name: &CString) -> Result<File, StageError> {
    let descriptor = openat(
        parent,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| "cache_tar_launcher_source_invalid")?;
    Ok(File::from(descriptor))
}

fn verify_source(
    source: &mut File,
    parent: &OwnedFd,
    name: &CString,
    before: &rustix::fs::Stat,
    expected_sha256: &str,
) -> Result<(), StageError> {
    validate_source_stat(before)?;
    if !same_directory_entry(parent, name, before)? {
        return Err("cache_tar_launcher_source_changed");
    }
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_launcher_read_failed")?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|_| "cache_tar_launcher_read_failed")?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| "cache_tar_launcher_size_invalid")?)
            .ok_or("cache_tar_launcher_size_invalid")?;
        if total > MAX_HELPER_BYTES {
            return Err("cache_tar_launcher_size_invalid");
        }
        digest.update(&buffer[..count]);
    }
    let after = rustix::fs::fstat(source.as_fd()).map_err(|_| "cache_tar_launcher_stat_failed")?;
    if !same_stat_version(before, &after) || !same_directory_entry(parent, name, &after)? {
        return Err("cache_tar_launcher_source_changed");
    }
    if lower_hex(&digest.finalize()) != expected_sha256 {
        return Err("cache_tar_launcher_digest_mismatch");
    }
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| "cache_tar_launcher_read_failed")?;
    Ok(())
}

fn validate_source_stat(stat: &rustix::fs::Stat) -> Result<(), StageError> {
    let mode = stat.st_mode & 0o7777;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != crate::process_owner::effective_uid()
        || mode & 0o111 == 0
        || mode & 0o7022 != 0
        || mode & 0o7000 != 0
        || stat.st_nlink != 1
        || stat.st_size <= 0
        || u64::try_from(stat.st_size).unwrap_or(u64::MAX) > MAX_HELPER_BYTES
    {
        return Err("cache_tar_launcher_source_owner_mode_or_type_invalid");
    }
    Ok(())
}

fn same_directory_entry(
    parent: &OwnedFd,
    name: &CString,
    file: &rustix::fs::Stat,
) -> Result<bool, StageError> {
    let path = statat(parent, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| "cache_tar_launcher_source_stat_failed")?;
    Ok(same_inode(file, &path) && file.st_mode == path.st_mode && file.st_uid == path.st_uid)
}

fn same_inode(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn same_identity_and_content(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    same_inode(left, right)
        && left.st_uid == right.st_uid
        && left.st_mode == right.st_mode
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
}

fn same_stat_version(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    same_identity_and_content(left, right)
        && left.st_nlink == right.st_nlink
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}

fn rollback_aliases(
    parent: &OwnedFd,
    aliases: &[CString],
    source: &rustix::fs::Stat,
) -> Result<(), StageError> {
    for alias in aliases {
        unlink_if_same(parent, alias, source)?;
    }
    Ok(())
}

fn unlink_if_same(
    parent: &OwnedFd,
    alias: &CString,
    source: &rustix::fs::Stat,
) -> Result<(), StageError> {
    if statat(parent, alias.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| same_inode(source, &stat))
    {
        rustix::fs::unlinkat(parent, alias.as_c_str(), AtFlags::empty())
            .map_err(|_| "cache_tar_launcher_rollback_failed")?;
    }
    Ok(())
}

fn canonical_runner_temp(path: &Path) -> Result<PathBuf, StageError> {
    if !path.is_absolute() || path.as_os_str().len() > 4_096 {
        return Err("cache_tar_launcher_runner_temp_invalid");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::Prefix(_)
        )
    }) {
        return Err("cache_tar_launcher_runner_temp_invalid");
    }
    let canonical =
        std::fs::canonicalize(path).map_err(|_| "cache_tar_launcher_runner_temp_invalid")?;
    if canonical == Path::new("/") || canonical.as_os_str().len() > 4_096 {
        return Err("cache_tar_launcher_runner_temp_invalid");
    }
    Ok(canonical)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
#[path = "../../tests/cache_action_stage_launcher_tests.rs"]
mod tests;
