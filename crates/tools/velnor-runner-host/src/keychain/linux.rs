//! Linux systemd credential source and private service credential checks.

use std::path::Path;
use zeroize::Zeroizing;

use super::{MAX_SECRET_LEN, read_secret};
use crate::HostError;

const CREDENTIAL_SOURCE: &str = "/etc/velnor-host/github-token";
const CREDENTIAL_DIRECTORY: &str = "/etc/velnor-host";

pub(super) fn store_configured(path: &Path, secret: &[u8]) -> Result<(), HostError> {
    if path != Path::new(CREDENTIAL_SOURCE) || current_uid()? != 0 {
        return Err(HostError::Keychain);
    }
    let group_id =
        velnor_runner_host_config::linux_service_group_id().map_err(|_| HostError::Keychain)?;
    velnor_runner_host_config::validate_linux_directory(Path::new(CREDENTIAL_DIRECTORY), group_id)
        .map_err(|_| HostError::Keychain)?;
    let policy = SecretFilePolicy {
        directory_uid: 0,
        directory_gid: group_id,
        directory_mode: 0o750,
        file_uid: 0,
        file_gid: 0,
        file_mode: 0o600,
    };
    store_secret_at(path, secret, &policy)
}

pub(super) fn remove_configured(path: &Path) -> Result<(), HostError> {
    if path != Path::new(CREDENTIAL_SOURCE) {
        return Err(HostError::Keychain);
    }
    let group_id =
        velnor_runner_host_config::linux_service_group_id().map_err(|_| HostError::Keychain)?;
    let policy = SecretFilePolicy {
        directory_uid: 0,
        directory_gid: group_id,
        directory_mode: 0o750,
        file_uid: 0,
        file_gid: 0,
        file_mode: 0o600,
    };
    remove_secret_at(path, &policy)
}

pub(super) fn load_configured() -> Result<Zeroizing<Vec<u8>>, HostError> {
    let directory = std::env::var_os("CREDENTIALS_DIRECTORY").ok_or(HostError::Keychain)?;
    load_systemd_credential(Path::new(&directory), "github-token")
}

pub(super) fn load_actions_read_token() -> Result<Zeroizing<Vec<u8>>, HostError> {
    let directory = std::env::var_os("CREDENTIALS_DIRECTORY").ok_or(HostError::Keychain)?;
    load_systemd_credential(Path::new(&directory), super::ACTIONS_READ_TOKEN_NAME)
}

#[derive(Clone, Copy)]
pub(super) struct SecretFilePolicy {
    directory_uid: u32,
    directory_gid: u32,
    directory_mode: u32,
    file_uid: u32,
    file_gid: u32,
    file_mode: u32,
}

fn store_secret_at(path: &Path, secret: &[u8], policy: &SecretFilePolicy) -> Result<(), HostError> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::MetadataExt;

    let parent = path.parent().ok_or(HostError::Keychain)?;
    validate_secret_directory(parent, policy)?;
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.uid() == policy.file_uid
                && metadata.gid() == policy.file_gid
                && metadata.mode() & 0o7777 == policy.file_mode => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) | Err(_) => return Err(HostError::Keychain),
    }
    let temporary = create_secret_temporary(path, secret)?;
    if secure_secret_temporary(&temporary, policy).is_err() {
        let _removed = fs::remove_file(&temporary);
        return Err(HostError::Keychain);
    }
    if fs::rename(&temporary, path).is_err() {
        let _removed = fs::remove_file(&temporary);
        return Err(HostError::Keychain);
    }
    if !file_has_owner_mode(path, policy) {
        let _removed = fs::remove_file(path);
        return Err(HostError::Keychain);
    }
    OpenOptions::new()
        .read(true)
        .open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Keychain)
}

fn create_secret_temporary(path: &Path, secret: &[u8]) -> Result<std::path::PathBuf, HostError> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or(HostError::Keychain)?;
    let name = path
        .file_name()
        .ok_or(HostError::Keychain)?
        .to_string_lossy();
    for _ in 0..8 {
        let number = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.tmp-{}-{number}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(mut file) => {
                if file.write_all(secret).is_err() || file.sync_all().is_err() {
                    let _removed = fs::remove_file(&candidate);
                    return Err(HostError::Keychain);
                }
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(HostError::Keychain),
        }
    }
    Err(HostError::Keychain)
}

fn secure_secret_temporary(path: &Path, policy: &SecretFilePolicy) -> Result<(), HostError> {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::PermissionsExt;

    velnor_runner_host_config::assign_owner(path, policy.file_uid, policy.file_gid)
        .map_err(|_| HostError::Keychain)?;
    fs::set_permissions(path, fs::Permissions::from_mode(policy.file_mode))
        .map_err(|_| HostError::Keychain)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| HostError::Keychain)?;
    file.sync_all().map_err(|_| HostError::Keychain)?;
    if file_has_owner_mode(path, policy) {
        Ok(())
    } else {
        Err(HostError::Keychain)
    }
}

fn remove_secret_at(path: &Path, policy: &SecretFilePolicy) -> Result<(), HostError> {
    use std::fs::{self, OpenOptions};

    let parent = path.parent().ok_or(HostError::Keychain)?;
    validate_secret_directory(parent, policy)?;
    if !path_has_owner_mode(path, policy)? {
        return if fs::symlink_metadata(path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            Ok(())
        } else {
            Err(HostError::Keychain)
        };
    }
    fs::remove_file(path).map_err(|_| HostError::Keychain)?;
    OpenOptions::new()
        .read(true)
        .open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Keychain)
}

fn validate_secret_directory(path: &Path, policy: &SecretFilePolicy) -> Result<(), HostError> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Keychain)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != policy.directory_uid
        || metadata.gid() != policy.directory_gid
        || metadata.mode() & 0o7777 != policy.directory_mode
    {
        return Err(HostError::Keychain);
    }
    Ok(())
}

fn path_has_owner_mode(path: &Path, policy: &SecretFilePolicy) -> Result<bool, HostError> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == policy.file_uid
            && metadata.gid() == policy.file_gid
            && metadata.mode() & 0o7777 == policy.file_mode),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(HostError::Keychain),
    }
}

fn file_has_owner_mode(path: &Path, policy: &SecretFilePolicy) -> bool {
    path_has_owner_mode(path, policy).is_ok_and(|matches| matches)
}

fn current_uid() -> Result<u32, HostError> {
    let output = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|_| HostError::Keychain)?;
    if !output.status.success() {
        return Err(HostError::Keychain);
    }
    std::str::from_utf8(&output.stdout)
        .map_err(|_| HostError::Keychain)?
        .trim()
        .parse()
        .map_err(|_| HostError::Keychain)
}

fn load_systemd_credential(directory: &Path, name: &str) -> Result<Zeroizing<Vec<u8>>, HostError> {
    let owner_uid = current_uid()?;
    let mut file = velnor_runner_host_config::open_systemd_credential_file(
        directory,
        name,
        owner_uid,
        MAX_SECRET_LEN,
    )
    .map_err(|_| HostError::Keychain)?;
    read_secret(&mut file).map_err(|_| HostError::Keychain)
}

#[cfg(test)]
pub(super) fn test_store(
    path: &Path,
    secret: &[u8],
    policy: &SecretFilePolicy,
) -> Result<(), HostError> {
    store_secret_at(path, secret, policy)
}

#[cfg(test)]
pub(super) fn test_remove(path: &Path, policy: &SecretFilePolicy) -> Result<(), HostError> {
    remove_secret_at(path, policy)
}

#[cfg(test)]
pub(super) fn test_load(directory: &Path, name: &str) -> Result<Zeroizing<Vec<u8>>, HostError> {
    load_systemd_credential(directory, name)
}

#[cfg(test)]
pub(super) fn test_load_actions_read_token(
    directory: &Path,
) -> Result<Zeroizing<Vec<u8>>, HostError> {
    load_systemd_credential(directory, super::ACTIONS_READ_TOKEN_NAME)
}

#[cfg(test)]
pub(super) fn test_arbitrary_store(path: &Path, secret: &[u8]) -> Result<(), HostError> {
    store_configured(path, secret)
}

#[cfg(test)]
pub(super) fn test_arbitrary_remove(path: &Path) -> Result<(), HostError> {
    remove_configured(path)
}

#[cfg(test)]
pub(super) fn test_policy(
    directory_uid: u32,
    directory_group: u32,
    directory_mode: u32,
    file_uid: u32,
    file_group: u32,
    file_mode: u32,
) -> SecretFilePolicy {
    SecretFilePolicy {
        directory_uid,
        directory_gid: directory_group,
        directory_mode,
        file_uid,
        file_gid: file_group,
        file_mode,
    }
}
