use super::*;
use serde::ser::{Error, Serializer};

fn identity() -> SessionIdentity {
    SessionIdentity::new(CommandRole::CargoTest, None, None, None).unwrap()
}

fn outcome() -> WorkloadResult {
    WorkloadResult {
        outcome: WorkloadOutcome::Succeeded,
        exit_code: Some(0),
    }
}

fn directory() -> (tempfile::TempDir, PathBuf) {
    #[cfg(unix)]
    let temporary = {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    };
    #[cfg(not(unix))]
    let temporary = tempfile::tempdir().unwrap();
    let canonical = temporary.path().canonicalize().unwrap();
    (temporary, canonical)
}

#[test]
fn nested_identity_retains_root_and_changes_parent() {
    let root = identity();
    let child = SessionIdentity::new(
        CommandRole::Exec,
        Some(&root.session_id),
        Some(&root.root_session_id),
        None,
    )
    .unwrap();
    let grandchild = SessionIdentity::new(
        CommandRole::CargoBuild,
        Some(&child.session_id),
        Some(&child.root_session_id),
        None,
    )
    .unwrap();
    assert_ne!(child.session_id, root.session_id);
    assert_ne!(grandchild.session_id, child.session_id);
    assert_eq!(
        grandchild.parent_session_id.as_ref(),
        Some(&child.session_id)
    );
    assert_eq!(grandchild.root_session_id, root.session_id);
    assert_eq!(child.child_environment()[0].1, child.session_id);
}

#[test]
fn invalid_or_incomplete_identity_is_rejected() {
    let root = identity();
    for (parent, root_id) in [
        (Some("../escape"), Some(root.session_id.as_str())),
        (Some(root.session_id.as_str()), None),
        (None, Some(root.session_id.as_str())),
        (Some(root.session_id.as_str()), Some("bad-root")),
    ] {
        assert!(SessionIdentity::new(CommandRole::Other, parent, root_id, None).is_err());
    }
    let (_temporary, path) = directory();
    let mut invalid = identity();
    invalid.session_id = "../escape".into();
    assert!(publish(&path, &invalid, outcome(), &()).is_err());
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
}

#[test]
fn public_correlation_groups_roots_and_is_inherited() {
    let token = "task:0123456789abcdef_safe-public";
    let root = SessionIdentity::new(CommandRole::CargoTest, None, None, Some(token)).unwrap();
    let independent =
        SessionIdentity::new(CommandRole::CargoBuild, None, None, Some(token)).unwrap();
    assert_ne!(root.root_session_id, independent.root_session_id);
    assert_eq!(root.caller_correlation, independent.caller_correlation);
    let environment = root.child_environment();
    assert_eq!(environment[2], (CORRELATION_ID_ENV, token));
    let child = SessionIdentity::new(
        CommandRole::Exec,
        Some(environment[0].1),
        Some(environment[1].1),
        Some(environment[2].1),
    )
    .unwrap();
    assert_eq!(child.caller_correlation.as_deref(), Some(token));
    assert_eq!(child.root_session_id, root.session_id);
    #[cfg(unix)]
    {
        let (_temporary, path) = directory();
        let report = publish(&path, &root, outcome(), &()).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
        assert_eq!(value["identity"]["caller_correlation"], token);
    }
}

#[test]
fn invalid_correlation_never_reaches_completed_reports() {
    for token in [
        "",
        "secret argument",
        "../private/path",
        "nonascii-é",
        "line\nbreak",
        &"a".repeat(129),
    ] {
        assert!(SessionIdentity::new(CommandRole::Other, None, None, Some(token)).is_err());
    }
    assert!(SessionIdentity::new(CommandRole::Other, None, None, Some(&"a".repeat(128))).is_ok());
    let (_temporary, path) = directory();
    let mut invalid = identity();
    invalid.caller_correlation = Some("forged/token".into());
    assert!(publish(&path, &invalid, outcome(), &()).is_err());
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn completed_report_has_supported_identity_and_role() {
    let (_temporary, path) = directory();
    let root = identity();
    let report = publish(&path, &root, outcome(), &serde_json::json!({"hits": 2})).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["completed"], true);
    assert_eq!(value["identity"]["session_id"], root.session_id);
    assert_eq!(value["identity"]["command_role"], "cargo_test");
    assert!(value["identity"]["parent_session_id"].is_null());
    assert_eq!(value["workload"]["outcome"], "succeeded");
    assert_eq!(value["statistics"]["hits"], 2);
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn duplicate_report_never_replaces_original() {
    let (_temporary, path) = directory();
    let root = identity();
    let report = publish(&path, &root, outcome(), &"original").unwrap();
    let original = std::fs::read(&report).unwrap();
    assert!(publish(&path, &root, outcome(), &"replacement").is_err());
    assert_eq!(std::fs::read(report).unwrap(), original);
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 1);
}

struct SerializationFailure;

impl Serialize for SerializationFailure {
    fn serialize<S: Serializer>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error> {
        Err(S::Error::custom("deliberate serialization failure"))
    }
}

#[test]
fn serialization_failure_leaves_no_partial_report() {
    let (_temporary, path) = directory();
    assert!(publish(&path, &identity(), outcome(), &SerializationFailure).is_err());
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
}

#[test]
fn relative_and_parent_alias_directories_are_rejected() {
    assert!(publish(Path::new("relative"), &identity(), outcome(), &()).is_err());
    let (_temporary, path) = directory();
    assert!(publish(&path.join("../alias"), &identity(), outcome(), &()).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_directory_and_report_destinations_are_rejected() {
    use std::os::unix::fs::symlink;
    let (_temporary, path) = directory();
    let actual = path.join("actual");
    let link = path.join("alias");
    std::fs::create_dir(&actual).unwrap();
    symlink(&actual, &link).unwrap();
    assert!(publish(&link, &identity(), outcome(), &()).is_err());
    assert!(publish(&link.join("child"), &identity(), outcome(), &()).is_err());
    let root = identity();
    let victim = path.join("victim");
    std::fs::write(&victim, b"original").unwrap();
    symlink(&victim, path.join(format!("{}.json", root.session_id))).unwrap();
    assert!(publish(&path, &root, outcome(), &()).is_err());
    assert_eq!(std::fs::read(victim).unwrap(), b"original");
}

#[cfg(unix)]
#[test]
fn unsafe_existing_directory_is_rejected_without_chmod() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (_temporary, path) = directory();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(publish(&path, &identity(), outcome(), &()).is_err());
    assert_eq!(std::fs::metadata(path).unwrap().mode() & 0o777, 0o755);
}

#[cfg(unix)]
#[test]
fn new_directory_is_private_and_published_file_read_only() {
    use std::os::unix::fs::MetadataExt;
    let (_temporary, path) = directory();
    let reports = path.join("reports");
    let report = publish(&reports, &identity(), outcome(), &()).unwrap();
    assert_eq!(std::fs::metadata(reports).unwrap().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(report).unwrap().mode() & 0o777, 0o400);
}

#[test]
fn nested_report_cannot_claim_to_be_its_own_root() {
    let root = identity();
    let mut nested = SessionIdentity::new(
        CommandRole::Exec,
        Some(&root.session_id),
        Some(&root.root_session_id),
        None,
    )
    .unwrap();
    nested.root_session_id.clone_from(&nested.session_id);
    assert!(nested.validate().is_err());
    let (_temporary, path) = directory();
    assert!(publish(&path, &nested, outcome(), &()).is_err());
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
}

#[cfg(not(unix))]
#[test]
fn unqualified_private_directory_policy_creates_no_report_directory() {
    let (_temporary, path) = directory();
    let reports = path.join("reports");
    let error = publish(&reports, &identity(), outcome(), &()).unwrap_err();
    assert!(error.to_string().contains("unqualified"));
    assert!(!reports.exists());
}
