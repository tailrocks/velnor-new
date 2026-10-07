use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::atomic::{FileOwner, FilePolicy};
use super::{service_identity_from_files, validate_owned_directory};
use crate::HostError;

#[test]
fn config_directory_rejects_wrong_group_mode_and_symlink() -> Result<(), String> {
    let directory = TempDir::new("directory-policy")?;
    let uid = command_id("-u")?;
    let gid = command_id("-g")?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;
    validate_owned_directory(directory.path(), policy(uid, gid, 0o750))
        .map_err(|error| error.to_string())?;
    if validate_owned_directory(directory.path(), policy(uid, gid.saturating_add(1), 0o750)).is_ok()
    {
        return Err("directory with the wrong service group was accepted".to_owned());
    }
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o770))
        .map_err(|error| error.to_string())?;
    if validate_owned_directory(directory.path(), policy(uid, gid, 0o750)).is_ok() {
        return Err("group-writable config directory was accepted".to_owned());
    }

    let link = directory.path().with_extension("link");
    symlink(directory.path(), &link).map_err(|error| error.to_string())?;
    if validate_owned_directory(&link, policy(uid, gid, 0o750)).is_ok() {
        return Err("config directory symlink was accepted".to_owned());
    }
    fs::remove_file(link).map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn policy(uid: u32, gid: u32, mode: u32) -> FilePolicy {
    FilePolicy {
        owner: FileOwner { uid, gid },
        mode,
    }
}

#[test]
fn service_identity_requires_unique_non_root_user_and_matching_primary_group() {
    let valid = service_identity_from_files(
        "root:x:0:0:root:/root:/bin/sh\nvelnor:x:994:994:Velnor:/var/lib/velnor:/usr/sbin/nologin\n",
        "root:x:0:\nvelnor:x:994:\n",
    );
    assert_eq!(valid, Ok(994));

    for (passwd, group) in [
        ("root:x:0:0:root:/root:/bin/sh\n", "root:x:0:\n"),
        (
            "velnor:x:0:994:Velnor:/var/lib/velnor:/usr/sbin/nologin\n",
            "velnor:x:994:\n",
        ),
        (
            "velnor:x:994:993:Velnor:/var/lib/velnor:/usr/sbin/nologin\n",
            "velnor:x:994:\n",
        ),
        (
            "velnor:x:994:994:Velnor:/var/lib/velnor:/usr/sbin/nologin\n",
            "velnor:x:994:\nvelnor:x:995:\n",
        ),
    ] {
        assert_eq!(
            service_identity_from_files(passwd, group),
            Err(HostError::Config)
        );
    }
}

pub(super) fn command_id(option: &str) -> Result<u32, String> {
    let output = std::process::Command::new("/usr/bin/id")
        .arg(option)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("could not read test user identity".to_owned());
    }
    String::from_utf8(output.stdout)
        .map_err(|error| error.to_string())?
        .trim()
        .parse()
        .map_err(|error| format!("invalid test identity: {error}"))
}

pub(super) struct TempDir(PathBuf);

impl TempDir {
    pub(super) fn new(label: &str) -> Result<Self, String> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-config-file-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

mod lifecycle_tests;
