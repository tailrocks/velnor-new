//! Host configuration file ownership and Linux service identity.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::HostPlatform;
use velnor_runner_journal::HostError;

mod atomic;
mod read;
mod secure_read;

/// Assign path ownership without dereferencing symlinks.
///
/// Re-exported for the host keychain, which stores the Linux service
/// credential under the same ownership rules as the config directory.
pub use atomic::assign_owner;
use atomic::{
    FileOwner, FilePolicy, publish_new_file, remove_file_with_policy, validate_owned_directory,
};
pub use read::{MAX_HOST_CONFIG_BYTES, read_host_config_bytes};
pub use secure_read::open_systemd_credential_file;

/// Linux system configuration path installed by the Debian package.
pub const LINUX_CONFIG_PATH: &str = "/etc/velnor-host/host.toml";

const LINUX_CONFIG_DIR: &str = "/etc/velnor-host";
const SERVICE_USER: &str = "velnor";
const SERVICE_GROUP: &str = "velnor";

/// Read a platform configuration after checking its file and directory owner.
/// Missing files return `Ok(None)`. Linux requires the package-owned system
/// path and the exact `root:velnor 0750/0640` directory and file modes.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the path, ownership, file type, or file
/// contents cannot be safely read.
pub fn read_host_config_file(
    path: &Path,
    platform: HostPlatform,
) -> Result<Option<String>, HostError> {
    read_host_config_bytes(path, platform)?
        .map(String::from_utf8)
        .transpose()
        .map_err(|_| HostError::Config)
}

/// Atomically create a new host configuration file without replacing an
/// existing path. Linux requires root and an installed `velnor` account/group.
///
/// # Errors
///
/// Returns [`HostError::Config`] when authority, ownership, permissions, or
/// atomic publication checks fail.
pub fn persist_host_config_file(
    path: &Path,
    text: &str,
    platform: HostPlatform,
) -> Result<(), HostError> {
    match platform {
        HostPlatform::Linux => persist_linux_config(path, text),
        HostPlatform::Macos => persist_macos_config(path, text),
    }
}

/// Check local authority and directory prerequisites before `connect` reads a
/// credential from stdin or contacts GitHub.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the requested target or local authority
/// does not match the platform contract.
pub fn validate_host_config_target(path: &Path, platform: HostPlatform) -> Result<(), HostError> {
    match platform {
        HostPlatform::Linux => {
            if path != Path::new(LINUX_CONFIG_PATH) || current_uid()? != 0 {
                return Err(HostError::Config);
            }
            validate_linux_directory(Path::new(LINUX_CONFIG_DIR), linux_service_group_id()?)
        }
        HostPlatform::Macos => Ok(()),
    }
}

/// Remove only an unchanged configuration with the supported owner and mode.
/// A missing file is idempotent; an unsafe or changed file is retained.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the path is unsafe or the file no longer
/// matches its expected contents, owner, or mode.
pub fn remove_host_config_file(
    path: &Path,
    expected_text: &str,
    platform: HostPlatform,
) -> Result<(), HostError> {
    match platform {
        HostPlatform::Linux => {
            if path != Path::new(LINUX_CONFIG_PATH) {
                return Err(HostError::Config);
            }
            let group_id = linux_service_group_id()?;
            remove_file_with_policy(
                path,
                expected_text,
                FilePolicy {
                    owner: FileOwner {
                        uid: 0,
                        gid: group_id,
                    },
                    mode: 0o640,
                },
                FilePolicy {
                    owner: FileOwner {
                        uid: 0,
                        gid: group_id,
                    },
                    mode: 0o750,
                },
            )
        }
        HostPlatform::Macos => remove_macos_config(path, expected_text),
    }
}

/// Resolve the dedicated Linux account's primary group from local system
/// account files. Missing, duplicate, root, or mismatched entries fail closed.
///
/// # Errors
///
/// Returns [`HostError::Config`] when account files are unreadable or do not
/// contain one matching non-root service account and group.
pub fn linux_service_group_id() -> Result<u32, HostError> {
    let passwd = fs::read_to_string("/etc/passwd").map_err(|_| HostError::Config)?;
    let group = fs::read_to_string("/etc/group").map_err(|_| HostError::Config)?;
    service_identity_from_files(&passwd, &group)
}

fn persist_linux_config(path: &Path, text: &str) -> Result<(), HostError> {
    if path != Path::new(LINUX_CONFIG_PATH) || current_uid()? != 0 {
        return Err(HostError::Config);
    }
    let group_id = linux_service_group_id()?;
    validate_linux_directory(Path::new(LINUX_CONFIG_DIR), group_id)?;
    publish_new_file(
        path,
        text,
        FileOwner {
            uid: 0,
            gid: group_id,
        },
        0o640,
    )
}

fn persist_macos_config(path: &Path, text: &str) -> Result<(), HostError> {
    let parent = path.parent().ok_or(HostError::Config)?;
    fs::create_dir_all(parent).map_err(|_| HostError::Config)?;
    let directory = fs::symlink_metadata(parent).map_err(|_| HostError::Config)?;
    if !safe_macos_directory(&directory)? {
        return Err(HostError::Config);
    }
    publish_new_file(
        path,
        text,
        FileOwner {
            uid: current_uid()?,
            gid: current_gid()?,
        },
        0o600,
    )
}

#[cfg(target_os = "macos")]
fn remove_macos_config(path: &Path, expected_text: &str) -> Result<(), HostError> {
    use std::os::unix::fs::MetadataExt;

    let parent = path.parent().ok_or(HostError::Config)?;
    let directory = fs::symlink_metadata(parent).map_err(|_| HostError::Config)?;
    if !safe_macos_directory(&directory)? {
        return Err(HostError::Config);
    }
    remove_file_with_policy(
        path,
        expected_text,
        FilePolicy {
            owner: FileOwner {
                uid: current_uid()?,
                gid: current_gid()?,
            },
            mode: 0o600,
        },
        FilePolicy {
            owner: FileOwner {
                uid: current_uid()?,
                gid: directory.gid(),
            },
            mode: directory.mode() & 0o7777,
        },
    )
}

#[cfg(not(target_os = "macos"))]
fn remove_macos_config(_path: &Path, _expected_text: &str) -> Result<(), HostError> {
    Err(HostError::Config)
}

/// Check the Linux service directory is root-owned with the service group.
///
/// The host keychain reuses this gate before storing the Linux service
/// credential next to the configuration file.
///
/// # Errors
///
/// Returns [`HostError::Config`] when the path is not a directory with
/// owner `root`, group `group_id`, and mode `0750`.
pub fn validate_linux_directory(path: &Path, group_id: u32) -> Result<(), HostError> {
    validate_owned_directory(
        path,
        FilePolicy {
            owner: FileOwner {
                uid: 0,
                gid: group_id,
            },
            mode: 0o750,
        },
    )
}

fn service_identity_from_files(passwd: &str, group: &str) -> Result<u32, HostError> {
    let mut groups = group
        .lines()
        .filter_map(|line| parse_named_id(line, SERVICE_GROUP, 2));
    let group_id = groups.next().ok_or(HostError::Config)??;
    if groups.next().is_some() || group_id == 0 {
        return Err(HostError::Config);
    }
    let mut users = passwd.lines().filter_map(parse_service_user);
    let (user_id, primary_group_id) = users.next().ok_or(HostError::Config)??;
    if users.next().is_some() || user_id == 0 || primary_group_id != group_id {
        return Err(HostError::Config);
    }
    Ok(group_id)
}

fn parse_named_id(line: &str, name: &str, index: usize) -> Option<Result<u32, HostError>> {
    let mut fields = line.split(':');
    (fields.next() == Some(name)).then(|| {
        fields
            .nth(index - 1)
            .ok_or(HostError::Config)?
            .parse::<u32>()
            .map_err(|_| HostError::Config)
    })
}

fn parse_service_user(line: &str) -> Option<Result<(u32, u32), HostError>> {
    let mut fields = line.split(':');
    (fields.next() == Some(SERVICE_USER)).then(|| {
        let uid = fields
            .nth(1)
            .ok_or(HostError::Config)?
            .parse::<u32>()
            .map_err(|_| HostError::Config)?;
        let gid = fields
            .next()
            .ok_or(HostError::Config)?
            .parse::<u32>()
            .map_err(|_| HostError::Config)?;
        Ok((uid, gid))
    })
}

#[cfg(target_os = "macos")]
fn safe_macos_directory(metadata: &fs::Metadata) -> Result<bool, HostError> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == current_uid()?
        && metadata.mode() & 0o022 == 0)
}

#[cfg(target_os = "macos")]
fn safe_macos_file(metadata: &fs::Metadata) -> Result<bool, HostError> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.uid() == current_uid()? && metadata.mode() & 0o7777 == 0o600)
}

#[cfg(not(target_os = "macos"))]
fn safe_macos_directory(_metadata: &fs::Metadata) -> Result<bool, HostError> {
    Err(HostError::Config)
}

#[cfg(not(target_os = "macos"))]
fn safe_macos_file(_metadata: &fs::Metadata) -> Result<bool, HostError> {
    Err(HostError::Config)
}

#[cfg(target_os = "linux")]
fn current_uid() -> Result<u32, HostError> {
    command_id("-u")
}

#[cfg(target_os = "linux")]
fn current_gid() -> Result<u32, HostError> {
    command_id("-g")
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<u32, HostError> {
    command_id("-u")
}

#[cfg(target_os = "macos")]
fn current_gid() -> Result<u32, HostError> {
    command_id("-g")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_uid() -> Result<u32, HostError> {
    Err(HostError::Config)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn current_gid() -> Result<u32, HostError> {
    Err(HostError::Config)
}

fn command_id(option: &str) -> Result<u32, HostError> {
    let output = Command::new("/usr/bin/id")
        .arg(option)
        .output()
        .map_err(|_| HostError::Config)?;
    if !output.status.success() {
        return Err(HostError::Config);
    }
    std::str::from_utf8(&output.stdout)
        .map_err(|_| HostError::Config)?
        .trim()
        .parse()
        .map_err(|_| HostError::Config)
}

#[cfg(test)]
mod tests;
