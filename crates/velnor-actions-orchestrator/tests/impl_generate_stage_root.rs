//! In-place generation keeps one private spare root across publication cycles.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use tempfile::tempdir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use super::impl_common::{TestResult, config_with_branch, make_repo};

#[test]
fn generate_stage_root_preserves_output_mode_and_retired_inode() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let target = root.join(".github");
    let local = target.join("local");
    let unmanaged = local.join("marker.txt");
    fs::create_dir_all(unmanaged.parent().ok_or("missing unmanaged parent")?)?;
    fs::write(&unmanaged, b"preserve across generation\n")?;
    fs::set_permissions(&local, fs::Permissions::from_mode(0o711))?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o751))?;
    let initial_root = fs::symlink_metadata(&target)?.ino();
    let spare = root.join(".github.velnor-stage/spare");
    let prep = prepare(root)?;
    let options = GenerateOptions::default();

    generate(&prep, &options)?;
    assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o7777, 0o751);
    assert_eq!(fs::metadata(&local)?.permissions().mode() & 0o7777, 0o711);
    assert_eq!(
        fs::read(target.join("local/marker.txt"))?,
        b"preserve across generation\n"
    );
    assert_eq!(fs::symlink_metadata(&spare)?.ino(), initial_root);
    assert!(fs::read_dir(&spare)?.next().is_none());
    let first_published_root = fs::symlink_metadata(&target)?.ino();

    generate(&prep, &options)?;
    assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o7777, 0o751);
    assert_eq!(fs::metadata(&local)?.permissions().mode() & 0o7777, 0o711);
    assert_eq!(
        fs::read(target.join("local/marker.txt"))?,
        b"preserve across generation\n"
    );
    assert_eq!(fs::symlink_metadata(&spare)?.ino(), first_published_root);
    assert!(fs::read_dir(&spare)?.next().is_none());
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[test]
fn generate_stage_root_preview_does_not_create_persistent_container() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let preview_parent = tempdir()?;
    let destination = preview_parent.path().join("preview");
    let prep = prepare(repo.path())?;
    let options = GenerateOptions {
        output_dir: Some(destination.clone()),
    };

    generate(&prep, &options)?;
    assert!(destination.join(".github").is_dir());
    assert!(!repo.path().join(".github.velnor-stage").exists());
    Ok(())
}
