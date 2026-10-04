use super::*;
use crate::catalog::rust_compiler_authority::RustCompilerArtifactAuthority;
use std::collections::BTreeSet;

#[test]
fn reviewed_source_inputs_do_not_require_or_grant_installed_receipts() {
    let source = RootRustCandidateSource::require_root_linux().expect("reviewed source inputs");
    assert_eq!(source.manifest().version(), "1.98.1");
    assert_eq!(source.manifest().components().len(), 5);
    let projection = source.projection();
    assert_eq!(projection["purpose"], "root-linux-candidate-artifact-v1");
    assert_eq!(projection["authority"], "source-inputs-only");
    assert!(projection.get("installation_receipt_sha256").is_none());
    assert!(projection.get("installed_tree_sha256").is_none());
    assert!(RustCompilerArtifactAuthority::require_root_linux().is_err());
}

#[test]
fn complete_native_transform_payload_has_unique_regular_paths() {
    let source = RootRustCandidateSource::require_root_linux().expect("reviewed source inputs");
    let payload = source.payload();
    let rows = payload.as_array().expect("payload array");
    let mut paths = BTreeSet::new();
    let components: BTreeSet<_> = source
        .manifest()
        .components()
        .iter()
        .map(|component| component.component())
        .collect();
    for row in rows {
        let path = row["path"].as_str().expect("path");
        assert!(paths.insert(path));
        assert!(!path.starts_with('/'));
        assert!(
            path.split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..")
        );
        assert!(components.contains(row["component"].as_str().expect("component")));
        assert_eq!(row["sha256"].as_str().expect("hash").len(), 64);
        assert!(matches!(row["mode"].as_u64(), Some(0o644 | 0o755)));
    }
    assert_eq!(rows.len(), 156);
    assert!(paths.contains("bin/rustc"));
    assert!(paths.contains("bin/cargo"));
}

#[test]
fn source_identity_binds_every_payload_field_and_launch_constraint() {
    let source = RootRustCandidateSource::require_root_linux().expect("reviewed source inputs");
    let original = source.identity_projection();
    let digest = source.qualification_digest();
    for field in [
        "path",
        "component",
        "archive_member",
        "size",
        "sha256",
        "mode",
    ] {
        let mut changed = original.clone();
        changed["payload"][0][field] = json!("changed");
        assert_ne!(sha256(changed.to_string().as_bytes()), digest, "{field}");
    }
    for field in [
        "transport",
        "permit_copy_rename",
        "automatic_install",
        "ambient_compiler_path",
        "legacy_manifest_fallback",
        "foundation_execution_admission",
    ] {
        let mut changed = original.clone();
        changed["constraints"][field] = json!("changed");
        assert_ne!(sha256(changed.to_string().as_bytes()), digest, "{field}");
    }
}
