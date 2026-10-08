use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

use rustix::fs::{Mode, OFlags, fchown, open};
use rustix::process::geteuid;

use super::super::*;

#[test]
fn accepts_the_packaged_service_state_parent_mode() -> Result<(), String> {
    let parent = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o750))
        .map_err(|error| error.to_string())?;

    let store = DiagnosticsStore::under_protected_parent(parent.path())
        .map_err(|error| error.to_string())?;
    let child = fs::symlink_metadata(parent.path().join("diagnostics"))
        .map_err(|error| error.to_string())?;
    let opened = store
        .root_directory
        .metadata()
        .map_err(|error| error.to_string())?;
    assert_eq!(child.uid(), geteuid().as_raw());
    assert_eq!(child.mode() & 0o7777, 0o700);
    assert_eq!((child.dev(), child.ino()), (opened.dev(), opened.ino()));
    Ok(())
}

#[test]
fn rejects_a_writable_ancestor_even_when_the_leaf_is_private() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let writable = root.path().join("writable");
    let leaf = writable.join("state");
    fs::create_dir(&writable).map_err(|error| error.to_string())?;
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o777))
        .map_err(|error| error.to_string())?;
    fs::create_dir(&leaf).map_err(|error| error.to_string())?;
    fs::set_permissions(&leaf, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;

    assert!(matches!(
        DiagnosticsStore::under_protected_parent(&leaf),
        Err(HostError::Path)
    ));
    Ok(())
}

#[test]
fn rejects_a_foreign_owned_ancestor() -> Result<(), String> {
    let service_uid = geteuid().as_raw();
    let foreign_uid = if service_uid == 65_534 {
        65_533
    } else {
        65_534
    };
    assert!(!super::super::filesystem::permitted_owner(
        foreign_uid,
        service_uid
    ));
    if geteuid().as_raw() != 0 {
        return Ok(());
    }
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let foreign = root.path().join("foreign");
    fs::create_dir(&foreign).map_err(|error| error.to_string())?;
    fs::set_permissions(&foreign, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let directory = open(
        &foreign,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| error.to_string())?;
    fchown(
        &directory,
        Some(rustix::fs::Uid::from_raw(foreign_uid)),
        None,
    )
    .map_err(|error| error.to_string())?;

    assert!(matches!(
        DiagnosticsStore::under_protected_parent(&foreign),
        Err(HostError::Path)
    ));
    Ok(())
}

#[test]
fn retained_root_descriptor_survives_path_replacement() -> Result<(), String> {
    let parent = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let store = DiagnosticsStore::under_protected_parent(parent.path())
        .map_err(|error| error.to_string())?;
    let original = parent.path().join("diagnostics-original");
    fs::rename(parent.path().join("diagnostics"), &original).map_err(|error| error.to_string())?;
    fs::create_dir(parent.path().join("diagnostics")).map_err(|error| error.to_string())?;
    fs::set_permissions(
        parent.path().join("diagnostics"),
        fs::Permissions::from_mode(0o700),
    )
    .map_err(|error| error.to_string())?;

    let identity = identity(91)?;
    let receipt = store
        .retain(&identity, &PostActionDisposition::NotRun, None)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        store.load(&identity, &PostActionDisposition::NotRun),
        Ok(Some(receipt))
    );
    assert!(original.join("launch-91/receipt.json").is_file());
    assert_eq!(
        fs::read_dir(parent.path().join("diagnostics"))
            .map_err(|error| error.to_string())?
            .count(),
        0
    );
    Ok(())
}

#[test]
fn receipt_symlink_is_rejected_without_following_it() -> Result<(), String> {
    let parent = tempfile::tempdir().map_err(|error| error.to_string())?;
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let store = DiagnosticsStore::under_protected_parent(parent.path())
        .map_err(|error| error.to_string())?;
    let identity = identity(92)?;
    store
        .retain(&identity, &PostActionDisposition::NotRun, None)
        .map_err(|error| error.to_string())?;
    let launch_dir = parent.path().join("diagnostics/launch-92");
    fs::remove_file(launch_dir.join("receipt.json")).map_err(|error| error.to_string())?;
    let outside = parent.path().join("outside.json");
    fs::write(&outside, b"forged receipt").map_err(|error| error.to_string())?;
    symlink(&outside, launch_dir.join("receipt.json")).map_err(|error| error.to_string())?;

    assert_eq!(
        store.load(&identity, &PostActionDisposition::NotRun),
        Err(HostError::Path)
    );
    assert_eq!(
        fs::read(outside).map_err(|error| error.to_string())?,
        b"forged receipt"
    );
    Ok(())
}

fn identity(launch_id: i64) -> Result<WorkerGenerationIdentity, String> {
    WorkerGenerationIdentity::new(
        launch_id,
        format!("velnor-{launch_id}"),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        None,
    )
    .map_err(|error| error.to_string())
}
