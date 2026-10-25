use std::{fs, path::Path, time::Duration};

use velnor_actions_mise::CheckDeadline;

use super::private_tempdir_in;
use crate::{OrchestratorError, filesystem_owner::held_directory::HeldDirectory};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(10)).expect("deadline")
}

#[test]
fn canonicalizes_symlinked_parent_before_private_owned_capture_and_cleanup() {
    let root = tempfile::tempdir().expect("root");
    let physical = root.path().join("physical");
    fs::create_dir(&physical).expect("physical parent");
    let alias = root.path().join("alias");
    directory_symlink(&physical, &alias);

    let owner = private_tempdir_in(&alias, "owned-release-").expect("owned tempdir");
    let physical = physical.canonicalize().expect("canonical physical parent");
    assert!(owner.path().starts_with(physical));
    assert_private_directory(owner.path());
    let path = owner.path().to_path_buf();
    fs::write(path.join("asset"), b"verified bytes").expect("asset");

    let held = HeldDirectory::capture_owned(owner, deadline()).expect("held owned directory");
    assert_eq!(
        held.read_file("asset", 64).expect("held asset"),
        b"verified bytes"
    );
    assert!(path.exists());
    drop(held);
    assert!(!path.exists(), "owned tempdir cleanup preserved");
}

#[test]
fn raw_alias_of_owned_temp_path_still_fails_held_file_link_component_check() {
    let root = tempfile::tempdir().expect("root");
    let physical = root.path().join("physical");
    fs::create_dir(&physical).expect("physical parent");
    let alias = root.path().join("alias");
    directory_symlink(&physical, &alias);

    let owner = tempfile::Builder::new()
        .prefix("owned-release-")
        .tempdir_in(&alias)
        .expect("raw aliased owner");
    let path = owner.path().to_path_buf();
    fs::write(path.join("asset"), b"verified bytes").expect("asset");
    assert!(matches!(
        HeldDirectory::capture_owned(owner, deadline()),
        Err(OrchestratorError::Internal { problem }) if problem == "check_path_symlink"
    ));
    assert!(!path.exists(), "rejected owner tempdir cleanup preserved");
}

#[test]
fn owned_asset_symlink_remains_rejected_and_owner_cleans_up() {
    let root = tempfile::tempdir().expect("root");
    let owner = private_tempdir_in(root.path(), "owned-release-").expect("owned tempdir");
    let path = owner.path().to_path_buf();
    fs::write(path.join("target"), b"asset bytes").expect("target");
    file_symlink(Path::new("target"), &path.join("asset"));

    assert!(HeldDirectory::capture_owned(owner, deadline()).is_err());
    assert!(!path.exists(), "failed owned capture drops its tempdir");
}

#[cfg(unix)]
fn assert_private_directory(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)
        .expect("tempdir metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}

#[cfg(windows)]
fn assert_private_directory(_path: &Path) {}

#[cfg(unix)]
fn directory_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("directory symlink");
}

#[cfg(windows)]
fn directory_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("directory symlink");
}

#[cfg(unix)]
fn file_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("file symlink");
}

#[cfg(windows)]
fn file_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link).expect("file symlink");
}
