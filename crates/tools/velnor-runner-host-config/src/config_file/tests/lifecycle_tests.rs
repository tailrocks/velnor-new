use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

use super::super::atomic::FileOwner;
use super::super::{publish_new_file, remove_file_with_policy, validate_owned_directory};
use super::{TempDir, command_id, policy};

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
