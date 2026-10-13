//! Atomic config publication and checked local file removal.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_runner_journal::HostError;

#[derive(Clone, Copy)]
pub(super) struct FileOwner {
    pub(super) uid: u32,
    pub(super) gid: u32,
}

#[derive(Clone, Copy)]
pub(super) struct FilePolicy {
    pub(super) owner: FileOwner,
    pub(super) mode: u32,
}

pub(super) fn publish_new_file(
    path: &Path,
    text: &str,
    owner: FileOwner,
    final_mode: u32,
) -> Result<(), HostError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or(HostError::Config)?;
    let file_name = path.file_name().ok_or(HostError::Config)?.to_string_lossy();
    let temporary = create_temporary(parent, &file_name, text, &SEQUENCE)?;
    if assign_owner(&temporary, owner.uid, owner.gid).is_err()
        || fs::set_permissions(&temporary, permissions(final_mode)).is_err()
        || sync_file(&temporary).is_err()
        || !path_has_owner_mode(&temporary, owner, final_mode)
    {
        let _removed = fs::remove_file(&temporary);
        return Err(HostError::Config);
    }
    if fs::hard_link(&temporary, path).is_err() {
        let _removed = fs::remove_file(&temporary);
        return Err(HostError::Config);
    }
    if fs::remove_file(&temporary).is_err() || sync_directory(parent).is_err() {
        let _removed_target = fs::remove_file(path);
        let _removed_temp = fs::remove_file(&temporary);
        let _synced = sync_directory(parent);
        return Err(HostError::Config);
    }
    Ok(())
}

fn create_temporary(
    parent: &Path,
    file_name: &str,
    text: &str,
    sequence: &AtomicU64,
) -> Result<std::path::PathBuf, HostError> {
    use std::os::unix::fs::OpenOptionsExt;

    for _ in 0..8 {
        let number = sequence.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{file_name}.tmp-{}-{number}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(mut file) => {
                if file.write_all(text.as_bytes()).is_err() || file.sync_all().is_err() {
                    let _removed = fs::remove_file(&candidate);
                    return Err(HostError::Config);
                }
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(HostError::Config),
        }
    }
    Err(HostError::Config)
}

pub(super) fn remove_file_with_policy(
    path: &Path,
    expected_text: &str,
    file_policy: FilePolicy,
    directory_policy: FilePolicy,
) -> Result<(), HostError> {
    let parent = path.parent().ok_or(HostError::Config)?;
    validate_owned_directory(parent, directory_policy)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(HostError::Config),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || !owned_mode(&metadata, file_policy)
        || fs::read_to_string(path).map_err(|_| HostError::Config)? != expected_text
    {
        return Err(HostError::Config);
    }
    fs::remove_file(path).map_err(|_| HostError::Config)?;
    sync_directory(parent)
}

pub(super) fn validate_owned_directory(path: &Path, policy: FilePolicy) -> Result<(), HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Config)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || !owned_mode(&metadata, policy) {
        return Err(HostError::Config);
    }
    Ok(())
}

fn owned_mode(metadata: &fs::Metadata, policy: FilePolicy) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.uid() == policy.owner.uid
        && metadata.gid() == policy.owner.gid
        && metadata.mode() & 0o7777 == policy.mode
}

fn path_has_owner_mode(path: &Path, owner: FileOwner, mode: u32) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && owned_mode(&metadata, FilePolicy { owner, mode })
    })
}

/// Assign `path` to `uid`:`gid` without dereferencing symlinks.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the ownership change fails.
pub fn assign_owner(path: &Path, uid: u32, gid: u32) -> Result<(), HostError> {
    #[cfg(target_os = "linux")]
    {
        let owner = format!("{uid}:{gid}");
        let status = Command::new("/usr/bin/chown")
            .args(["--no-dereference", owner.as_str(), "--"])
            .arg(path)
            .status()
            .map_err(|_| HostError::Config)?;
        status.success().then_some(()).ok_or(HostError::Config)
    }
    #[cfg(target_os = "macos")]
    {
        let _identity = (uid, gid);
        let _path = path;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _identity = (path, uid, gid);
        Err(HostError::Config)
    }
}

fn sync_file(path: &Path) -> Result<(), HostError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| HostError::Config)
}

pub(super) fn sync_directory(path: &Path) -> Result<(), HostError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Config)
}

fn permissions(mode: u32) -> fs::Permissions {
    use std::os::unix::fs::PermissionsExt;
    fs::Permissions::from_mode(mode)
}
