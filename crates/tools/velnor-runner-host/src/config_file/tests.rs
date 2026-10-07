use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::atomic::{FileOwner, FilePolicy};
use super::{
    publish_new_file, remove_file_with_policy, service_identity_from_files,
    validate_owned_directory,
};
use crate::HostError;

#[test]
fn atomic_config_publish_sets_group_and_mode_before_link_and_never_replaces() -> Result<(), String>
{
    let directory = TempDir::new("atomic-publish")?;
    let uid = command_id("-u")?;
    let gid = command_id("-g")?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;
    validate_owned_directory(directory.path(), policy(uid, gid, 0o750))
        .map_err(|error| error.to_string())?;

    let path = directory.path().join("host.toml");
    publish_new_file(&path, "schema = 1\n", FileOwner { uid, gid }, 0o640)
        .map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.uid() != uid || metadata.gid() != gid || metadata.mode() & 0o7777 != 0o640 {
        return Err("published config owner or mode is incorrect".to_owned());
    }
    if fs::read_to_string(&path).map_err(|error| error.to_string())? != "schema = 1\n" {
        return Err("published config content changed".to_owned());
    }
    let leftovers = fs::read_dir(directory.path())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().contains(".tmp-"));
    if leftovers {
        return Err("temporary config path remained after publication".to_owned());
    }
    if publish_new_file(&path, "schema = 2\n", FileOwner { uid, gid }, 0o640).is_ok() {
        return Err("config publication replaced an existing file".to_owned());
    }
    if fs::read_to_string(&path).map_err(|error| error.to_string())? != "schema = 1\n" {
        return Err("failed publication changed the existing config".to_owned());
    }
    Ok(())
}

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

#[test]
fn config_removal_requires_exact_contents_owner_mode_and_regular_file() -> Result<(), String> {
    let directory = TempDir::new("safe-removal")?;
    let uid = command_id("-u")?;
    let gid = command_id("-g")?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;
    let path = directory.path().join("host.toml");
    publish_new_file(
        &path,
        "managed_by = \"velnor-host-connect-v1\"\n",
        FileOwner { uid, gid },
        0o640,
    )
    .map_err(|error| error.to_string())?;

    if remove_file_with_policy(
        &path,
        "managed_by = \"operator\"\n",
        policy(uid, gid, 0o640),
        policy(uid, gid, 0o750),
    )
    .is_ok()
        || !path.exists()
    {
        return Err("changed config was removed".to_owned());
    }
    remove_file_with_policy(
        &path,
        "managed_by = \"velnor-host-connect-v1\"\n",
        policy(uid, gid, 0o640),
        policy(uid, gid, 0o750),
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        return Err("exact managed config was not removed".to_owned());
    }

    let target = directory.path().join("target.toml");
    fs::write(&target, "managed\n").map_err(|error| error.to_string())?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640))
        .map_err(|error| error.to_string())?;
    let symlink_path = directory.path().join("symlink.toml");
    symlink(&target, &symlink_path).map_err(|error| error.to_string())?;
    if remove_file_with_policy(
        &symlink_path,
        "managed\n",
        policy(uid, gid, 0o640),
        policy(uid, gid, 0o750),
    )
    .is_ok()
        || !target.exists()
        || !symlink_path.exists()
    {
        return Err("symlink config removal was accepted".to_owned());
    }
    Ok(())
}

fn policy(uid: u32, gid: u32, mode: u32) -> FilePolicy {
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

fn command_id(option: &str) -> Result<u32, String> {
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

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, String> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-config-file-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
