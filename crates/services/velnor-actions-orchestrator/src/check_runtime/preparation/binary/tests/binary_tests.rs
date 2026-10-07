use super::*;

#[test]
fn wrong_digest_refuses_wrapper_without_invocation_or_copy() {
    let temp = tempfile::TempDir::new().expect("temp");
    let marker = temp.path().join("wrapper-ran");
    let source = temp.path().join("mise");
    let script = format!(
        "#!/bin/sh\nprintf invoked > '{}'\nprintf '2026.9.18\\n'\n",
        marker.display()
    );
    std::fs::write(&source, script).expect("wrapper");
    let destination = temp.path().join("owned-mise");
    let error = project_binary(
        &source,
        &destination,
        MISE_BINARY_SHA256_MACOS_ARM64,
        mise_binary_pin(CheckPlatform::MacosArm64).max_bytes,
    )
    .expect_err("unqualified");
    assert!(error.to_string().contains("mise_binary_unqualified_sha256"));
    assert!(!destination.exists());
    assert!(!marker.exists(), "unqualified bytes are never executed");
}
#[test]
fn qualified_fixture_bytes_copy_exclusively_and_survive_source_replacement() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("source");
    let destination = temp.path().join("owned");
    let bytes = b"qualified fixture bytes";
    let expected = velnor_actions_orchestrator_core::sha256::sha256_hex(bytes);
    verify_binary_bytes(bytes, &expected).expect("positive identity");
    std::fs::write(&source, bytes).expect("source");
    project_binary(&source, &destination, &expected, 1024).expect("projection");
    std::fs::write(&source, b"replacement").expect("replace ambient source");
    assert_eq!(std::fs::read(&destination).expect("owned bytes"), bytes);
    assert!(
        !std::fs::symlink_metadata(&destination)
            .expect("meta")
            .file_type()
            .is_symlink()
    );
    assert!(project_binary(&source, &destination, &expected, 1024).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&destination)
                .expect("mode")
                .permissions()
                .mode()
                & 0o777,
            0o500
        );
    }
}
#[cfg(unix)]
#[test]
fn source_symlink_and_existing_destination_refuse() {
    let temp = tempfile::TempDir::new().expect("temp");
    let bytes = b"fixture";
    let expected = velnor_actions_orchestrator_core::sha256::sha256_hex(bytes);
    let source = temp.path().join("source");
    std::fs::write(&source, bytes).expect("source");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&source, &link).expect("link");
    assert!(project_binary(&link, &temp.path().join("owned"), &expected, 1024).is_err());
    let destination = temp.path().join("existing");
    std::fs::write(&destination, b"original").expect("existing");
    assert!(project_binary(&source, &destination, &expected, 1024).is_err());
    assert_eq!(std::fs::read(destination).expect("unchanged"), b"original");
}
