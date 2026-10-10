use super::*;
use std::error::Error;
use std::process::Command;

use tempfile::tempdir;

#[test]
#[cfg(unix)]
fn generate_stage_root_creates_private_self_ignored_state() -> Result<(), Box<dyn Error>> {
    let repo = tempdir()?;
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repo.path())
        .status()?;
    assert!(init.success());

    let store = StageRoot::open(repo.path())?;
    assert_eq!(read_mode(&store.container), PRIVATE_DIR_MODE);
    assert_eq!(read_mode(&store.spare), PRIVATE_DIR_MODE);
    assert_eq!(fs::read(store.container.join(IGNORE_FILE))?, b"*\n");
    assert_eq!(
        read_mode(&store.container.join(IGNORE_FILE)),
        PRIVATE_FILE_MODE
    );
    assert_eq!(
        read_mode(&store.container.join(OWNER_FILE)),
        PRIVATE_FILE_MODE
    );

    let status = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(repo.path())
        .output()?;
    assert!(status.status.success());
    assert!(status.stdout.is_empty(), "{:?}", status.stdout);
    for path in [
        format!("{CONTAINER}/{IGNORE_FILE}"),
        format!("{CONTAINER}/{OWNER_FILE}"),
    ] {
        let ignored = Command::new("git")
            .args(["check-ignore", "-v", &path])
            .current_dir(repo.path())
            .output()?;
        assert!(ignored.status.success(), "{path}: {:?}", ignored.stderr);
        assert!(String::from_utf8(ignored.stdout)?.contains("*"));
    }
    let reopened = StageRoot::open(repo.path())?;
    assert_eq!(store.container_identity, reopened.container_identity);
    assert_eq!(store.spare_identity, reopened.spare_identity);
    Ok(())
}

#[test]
fn generate_stage_root_cleans_children_without_replacing_spare_inode() -> Result<(), Box<dyn Error>>
{
    let repo = tempdir()?;
    let store = StageRoot::open(repo.path())?;
    let root_identity = store.spare_identity;
    fs::create_dir(store.spare.join("stale-directory"))?;
    fs::write(store.spare.join("stale-directory/partial"), b"partial")?;
    store.clear_spare_before_staging()?;

    let (after, metadata) = FsIdentity::capture(&store.spare)?;
    assert_eq!(after, root_identity);
    assert_eq!(metadata.permissions().readonly(), false);
    assert!(fs::read_dir(&store.spare)?.next().is_none());
    assert!(store.spare.is_dir());
    Ok(())
}

#[test]
fn generate_stage_root_rejects_owner_record_from_another_worktree() -> Result<(), Box<dyn Error>> {
    let parent = tempdir()?;
    let first_root = parent.path().join("first");
    let second_root = parent.path().join("second");
    fs::create_dir(&first_root)?;
    fs::create_dir(&second_root)?;
    let target = second_root.join(".github");
    fs::create_dir(&target)?;
    fs::write(target.join("keep"), b"unchanged")?;
    let target_identity = FsIdentity::capture(&target)?.0;
    let first = StageRoot::open(&first_root)?;
    let foreign_container = second_root.join(CONTAINER);
    fs::rename(&first.container, &foreign_container)?;

    let error = StageRoot::open(&second_root).expect_err("foreign owner must fail");
    assert!(
        error.to_string().contains("owner_record_mismatch"),
        "{error}"
    );
    assert!(foreign_container.join(SPARE_DIR).is_dir());
    assert_eq!(FsIdentity::capture(&target)?.0, target_identity);
    assert_eq!(fs::read(target.join("keep"))?, b"unchanged");
    Ok(())
}

#[test]
fn generate_stage_root_reopens_crash_residue_and_keeps_root() -> Result<(), Box<dyn Error>> {
    let repo = tempdir()?;
    let store = StageRoot::open(repo.path())?;
    let before = store.spare_identity;
    fs::write(store.spare.join("interrupted-write"), b"partial")?;
    drop(store);

    let resumed = StageRoot::open(repo.path())?;
    resumed.clear_spare_before_staging()?;
    let (after, _) = FsIdentity::capture(&resumed.spare)?;
    assert_eq!(after, before);
    assert!(fs::read_dir(&resumed.spare)?.next().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn generate_stage_root_rejects_foreign_owned_output_directories() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let repo = tempdir()?;
    let target = repo.path().join(".github");
    let nested = target.join("nested");
    fs::create_dir_all(&nested)?;
    fs::write(nested.join("marker"), b"keep")?;
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o777))?;
    let store = StageRoot::open(repo.path())?;
    let target_identity = store.validate_target(&target)?;
    let current_owner = super::fs_ops::metadata_owner(&fs::metadata(repo.path())?)
        .ok_or("Unix owner metadata unavailable")?;
    let effective_owner = rustix::process::geteuid().as_raw();
    assert_eq!(current_owner, effective_owner);
    let foreign_owner = if current_owner == 0 { 65_534 } else { 0 };
    let changed = rustix::fs::chown(
        &nested,
        Some(rustix::process::Uid::from_raw(foreign_owner)),
        None,
    );

    if changed.is_ok() {
        let actual_owner = super::fs_ops::metadata_owner(&fs::symlink_metadata(&nested)?)
            .ok_or("Unix owner metadata unavailable")?;
        assert_eq!(actual_owner, foreign_owner);
        eprintln!(
            "foreign-owner fixture: effective_uid={effective_owner}, directory_uid={actual_owner}"
        );
        let error = store
            .validate_target_directories(&target, target_identity)
            .expect_err("foreign descendant owner must fail before publication");
        assert!(
            error.to_string().contains("foreign_directory_owner"),
            "{error}"
        );
    } else {
        eprintln!(
            "foreign-owner fixture unavailable: effective_uid={effective_owner}; exercising owner predicate only"
        );
        let error = super::fs_ops::validate_directory_owners(&nested, Some(foreign_owner))
            .expect_err("mismatched owner predicate must fail closed");
        assert!(
            error.to_string().contains("foreign_directory_owner"),
            "{error}"
        );
    }
    assert_eq!(fs::read(nested.join("marker"))?, b"keep");
    Ok(())
}

#[cfg(unix)]
#[test]
fn generate_stage_root_refuses_symlink_collision_and_insecure_mode() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let parent = tempdir()?;
    let root = parent.path().join("repo");
    let outside = parent.path().join("outside");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    fs::write(outside.join("marker"), b"keep")?;
    symlink(&outside, root.join(CONTAINER))?;
    assert!(StageRoot::open(&root).is_err());
    assert_eq!(fs::read(outside.join("marker"))?, b"keep");

    let secure_root = parent.path().join("secure");
    fs::create_dir(&secure_root)?;
    let store = StageRoot::open(&secure_root)?;
    fs::set_permissions(&store.container, fs::Permissions::from_mode(0o755))?;
    assert!(StageRoot::open(&secure_root).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn generate_stage_root_cleanup_unlinks_child_symlinks_without_following()
-> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let parent = tempdir()?;
    let root = parent.path().join("repo");
    let outside = parent.path().join("outside");
    fs::create_dir(&root)?;
    fs::create_dir(&outside)?;
    fs::write(outside.join("marker"), b"keep")?;
    let store = StageRoot::open(&root)?;
    symlink(outside.join("marker"), store.spare.join("file-link"))?;
    symlink(&outside, store.spare.join("directory-link"))?;

    store.clear_spare_before_staging()?;
    assert_eq!(fs::read(outside.join("marker"))?, b"keep");
    assert!(fs::read_dir(&store.spare)?.next().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn generate_stage_root_device_guard_rejects_different_devices() {
    let first = FsIdentity {
        device: 2,
        inode: 4,
    };
    let same = FsIdentity {
        device: 2,
        inode: 9,
    };
    let other = FsIdentity {
        device: 3,
        inode: 9,
    };
    assert!(super::fs_ops::same_device(first, same));
    assert!(!super::fs_ops::same_device(first, other));
}

#[cfg(unix)]
fn read_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o7777
}
