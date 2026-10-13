use std::io::{Cursor, Read};
use std::os::unix::fs::symlink;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use super::*;

mod filesystem_tests;
mod protected_state_tests;

#[test]
fn redaction_removes_sensitive_lines_and_long_token_values() -> Result<(), String> {
    let marker = "synthetic-jit-marker-0123456789abcdef";
    let input =
        format!("normal runner diagnostic\nAuthorization: Bearer {marker}\ntrace={marker}\n");
    let redacted = redact_log(input.as_bytes()).map_err(|error| error.to_string())?;
    let text = String::from_utf8(redacted).map_err(|error| error.to_string())?;
    assert!(!text.contains(marker));
    assert!(text.contains("normal runner diagnostic"));
    assert!(text.contains("[REDACTED sensitive diagnostic line]"));
    assert!(text.contains("[REDACTED token]"));
    Ok(())
}

#[test]
fn archive_sanitizer_flattens_paths_and_discards_non_log_files() -> Result<(), String> {
    let archive = archive(&[
        ("_diag/Runner_1.log", b"runner trace\n"),
        ("config.json", b"secret"),
    ]);
    let sanitized = sanitize_archive(&archive).map_err(|error| error.to_string())?;
    let names = tar_names(&sanitized)?;
    assert_eq!(names, vec!["runner-001.log"]);
    Ok(())
}

#[test]
fn archive_sanitizer_rejects_links_and_traversal() {
    let linked = archive_with_link();
    assert_eq!(sanitize_archive(&linked), Err(HostError::Docker));

    let mut traversing = archive(&[("credential.log", b"no")]);
    rewrite_first_path(&mut traversing, b"../credential.log");
    assert_eq!(sanitize_archive(&traversing), Err(HostError::Docker));
}

#[test]
fn receipt_recovery_rejects_a_symlinked_generation_directory() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let target = tempfile::tempdir().map_err(|error| error.to_string())?;
    symlink(target.path(), root.path().join("launch-41")).map_err(|error| error.to_string())?;
    let store = private_store(&root)?;
    let identity = WorkerGenerationIdentity::new(
        41,
        "velnor-41".to_owned(),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        None,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        store.load(&identity, &PostActionDisposition::NotRun),
        Err(HostError::Path)
    );
    Ok(())
}

#[test]
fn receipt_recovery_binds_source_absence_to_not_run_disposition() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = private_store(&root)?;
    let identity = WorkerGenerationIdentity::new(
        42,
        "velnor-42".to_owned(),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        None,
    )
    .map_err(|error| error.to_string())?;
    let not_run = PostActionDisposition::NotRun;
    let receipt = store
        .retain(&identity, &not_run, None)
        .map_err(|error| error.to_string())?;
    assert!(receipt.source_absent());
    assert_eq!(receipt.relative_path(), "");
    assert_eq!(receipt.sha256(), digest_hex(&[]));
    assert_eq!(receipt.bytes(), 0);
    assert!(
        !root
            .path()
            .join("launch-42/runner-diagnostics.tar")
            .exists()
    );
    assert_eq!(store.load(&identity, &not_run), Ok(Some(receipt)));
    assert_eq!(
        store.load(&identity, &PostActionDisposition::Unknown),
        Err(HostError::Identity)
    );
    assert_eq!(
        store.retain(
            &identity,
            &not_run,
            Some(&archive(&[("Runner.log", b"not run")]))
        ),
        Err(HostError::Identity)
    );
    Ok(())
}

#[test]
fn never_started_receipt_refuses_an_untracked_archive() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let store = private_store(&root)?;
    let identity = WorkerGenerationIdentity::new(
        43,
        "velnor-43".to_owned(),
        "w0123456789abcdef0123456789abcdef".to_owned(),
        "a123456789abcdef".to_owned(),
        "b123456789abcdef".to_owned(),
        None,
    )
    .map_err(|error| error.to_string())?;
    let launch_directory = root.path().join("launch-43");
    std::fs::create_dir(&launch_directory).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&launch_directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    std::fs::write(
        launch_directory.join("runner-diagnostics.tar"),
        b"untracked",
    )
    .map_err(|error| error.to_string())?;

    assert_eq!(
        store.retain(&identity, &PostActionDisposition::NotRun, None),
        Err(HostError::Identity)
    );
    Ok(())
}

#[test]
fn diagnostics_store_rejects_group_readable_root() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        DiagnosticsStore::new(root.path()),
        Err(HostError::Path)
    ));
    Ok(())
}

#[test]
fn diagnostics_store_is_created_only_below_an_existing_private_parent() -> Result<(), String> {
    let parent = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;

    let store = DiagnosticsStore::under_protected_parent(parent.path())
        .map_err(|error| error.to_string())?;
    let child = parent.path().join("diagnostics");
    let metadata = std::fs::symlink_metadata(&child).map_err(|error| error.to_string())?;
    assert!(metadata.is_dir());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::read_dir(parent.path())
            .map_err(|error| error.to_string())?
            .count(),
        1
    );

    let reopened = DiagnosticsStore::under_protected_parent(parent.path())
        .map_err(|error| error.to_string())?;
    let first = store
        .root_directory
        .metadata()
        .map_err(|error| error.to_string())?;
    let second = reopened
        .root_directory
        .metadata()
        .map_err(|error| error.to_string())?;
    assert_eq!((first.dev(), first.ino()), (second.dev(), second.ino()));
    Ok(())
}

#[test]
fn diagnostics_store_rejects_missing_insecure_and_symlinked_parents() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let missing = root.path().join("missing");
    assert!(matches!(
        DiagnosticsStore::under_protected_parent(&missing),
        Err(HostError::Path)
    ));

    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o770))
        .map_err(|error| error.to_string())?;
    assert!(matches!(
        DiagnosticsStore::under_protected_parent(root.path()),
        Err(HostError::Path)
    ));
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;

    let secure = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::set_permissions(secure.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let alias = root.path().join("parent-alias");
    symlink(secure.path(), &alias).map_err(|error| error.to_string())?;
    assert!(matches!(
        DiagnosticsStore::under_protected_parent(&alias),
        Err(HostError::Path)
    ));
    Ok(())
}

#[test]
fn diagnostics_store_rejects_existing_symlink_or_insecure_child() -> Result<(), String> {
    let target = tempfile::tempdir().map_err(|error| error.to_string())?;
    let parent = tempfile::tempdir().map_err(|error| error.to_string())?;
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    symlink(target.path(), parent.path().join("diagnostics")).map_err(|error| error.to_string())?;
    assert!(matches!(
        DiagnosticsStore::under_protected_parent(parent.path()),
        Err(HostError::Path)
    ));

    std::fs::remove_file(parent.path().join("diagnostics")).map_err(|error| error.to_string())?;
    std::fs::create_dir(parent.path().join("diagnostics")).map_err(|error| error.to_string())?;
    std::fs::set_permissions(
        parent.path().join("diagnostics"),
        std::fs::Permissions::from_mode(0o755),
    )
    .map_err(|error| error.to_string())?;
    assert!(matches!(
        DiagnosticsStore::under_protected_parent(parent.path()),
        Err(HostError::Path)
    ));
    Ok(())
}

fn private_store(root: &tempfile::TempDir) -> Result<DiagnosticsStore, String> {
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    DiagnosticsStore::new(root.path()).map_err(|error| error.to_string())
}

fn rewrite_first_path(archive: &mut [u8], path: &[u8]) {
    assert!(archive.len() >= 512);
    assert!(path.len() < 100);
    archive[..100].fill(0);
    archive[..path.len()].copy_from_slice(path);
    archive[148..156].fill(b' ');
    let checksum = archive[..512]
        .iter()
        .map(|byte| u64::from(*byte))
        .sum::<u64>();
    let checksum_field = format!("{checksum:06o}\0 ");
    assert_eq!(checksum_field.len(), 8);
    archive[148..156].copy_from_slice(checksum_field.as_bytes());
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut builder = Builder::new(&mut bytes);
    for (name, value) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_size(u64::try_from(value.len()).expect("test entry size fits in u64"));
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *value)
            .expect("append");
    }
    builder.finish().expect("finish");
    drop(builder);
    bytes
}

fn archive_with_link() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut builder = Builder::new(&mut bytes);
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Symlink);
    header.set_size(0);
    header.set_link_name("/etc/shadow").expect("link target");
    header.set_cksum();
    builder
        .append_data(&mut header, "runner.log", &[][..])
        .expect("link");
    builder.finish().expect("finish");
    drop(builder);
    bytes
}

fn tar_names(bytes: &[u8]) -> Result<Vec<String>, String> {
    Archive::new(Cursor::new(bytes))
        .entries()
        .map_err(|error| error.to_string())?
        .map(|entry| {
            let mut entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path().map_err(|error| error.to_string())?;
            let name = path
                .to_str()
                .ok_or_else(|| "non-utf8 tar path".to_owned())?
                .to_owned();
            let mut discard = Vec::new();
            entry
                .read_to_end(&mut discard)
                .map_err(|error| error.to_string())?;
            Ok(name)
        })
        .collect()
}
