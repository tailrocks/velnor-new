//! Typed transport and metadata input regression evidence.

use std::path::Path;

use velnor_actions_rust::{
    DepKind, LocalEdge, PackageRecord, SkippedPathEdge, TargetRecord, WorkspaceRecord,
};

use super::{
    AnalysisIdentity, AnalysisSource, authority::RemoteAnalysisAuthority, build_payload,
    parse_authenticated, resolution_inputs_digest,
};

#[path = "analysis_inventory_canonical_tests.rs"]
mod canonical;

fn fixture(root: &Path) -> (Vec<String>, Vec<(String, WorkspaceRecord)>) {
    let root = root.canonicalize().expect("canonical root");
    let root = root.as_path();
    std::fs::create_dir_all(root.join("member/src")).expect("source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['member']\nresolver='3'\n",
    )
    .expect("workspace");
    std::fs::write(
        root.join("member/Cargo.toml"),
        "[package]\nname='member'\nversion='1.0.0'\nedition='2024'\n",
    )
    .expect("member");
    std::fs::write(root.join("member/src/lib.rs"), "pub fn example() {}\n").expect("source");
    std::fs::write(root.join("README.md"), "documentation\n").expect("docs");
    let id = format!("path+file://{}/member#member@1.0.0", root.display());
    let record = WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![id.clone()],
        packages: vec![PackageRecord {
            id: id.clone(),
            name: "member".to_owned(),
            version: "1.0.0".to_owned(),
            manifest: "member/Cargo.toml".to_owned(),
            external: false,
            in_workspace: true,
            targets: vec![TargetRecord {
                kind: "proc-macro".to_owned(),
                name: "member".to_owned(),
                test: true,
                doctest: false,
                required_features: vec!["feature".to_owned()],
            }],
            features: vec!["feature".to_owned()],
            has_build_script: true,
        }],
        edges: [DepKind::Normal, DepKind::Build, DepKind::Dev]
            .into_iter()
            .map(|kind| LocalEdge {
                from: id.clone(),
                to: id.clone(),
                kind,
                optional: true,
                target: Some("cfg(unix)".to_owned()),
            })
            .collect(),
        skipped_edges: vec![SkippedPathEdge {
            from: id,
            path: root.join("other").display().to_string(),
            kind: DepKind::Dev,
            optional: true,
            target: Some("cfg(windows)".to_owned()),
        }],
    };
    let files = [
        "Cargo.toml",
        "member/Cargo.toml",
        "member/src/lib.rs",
        "README.md",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    (files, vec![("Cargo.toml".to_owned(), record)])
}

fn identity(digest: String) -> AnalysisIdentity {
    AnalysisIdentity {
        helper_sha256: "a".repeat(64),
        cargo_identity: super::QUALIFIED_CARGO_TEST_IDENTITY.to_owned(),
        cargo_pin: velnor_actions_mise::ToolCatalog::pinned().rustup_toolchain(),
        resolution_inputs_digest: digest,
        source: AnalysisSource {
            repository: "owner/repo".to_owned(),
            head_sha: "b".repeat(40),
            workflow_sha: "c".repeat(40),
            run_id: 42,
            run_attempt: 2,
            branch: "main".to_owned(),
        },
    }
}

#[test]
fn complete_typed_inventory_roundtrips_and_relocates() {
    let old = tempfile::tempdir().expect("old root");
    let new = tempfile::tempdir().expect("new root");
    let (files, records) = fixture(old.path());
    let (_, relocated) = fixture(new.path());
    let identity =
        identity(resolution_inputs_digest(old.path(), &files, &records).expect("digest"));
    let payload = build_payload(old.path(), identity.clone(), &records).expect("payload");
    assert!(!payload.contains(&old.path().display().to_string()));
    let authority = RemoteAnalysisAuthority::fixture(identity, &payload);
    let proof = parse_authenticated(new.path(), &files, &payload, &authority).expect("proof");
    for (manifest, record) in &relocated {
        assert_eq!(proof.record_for(manifest), Some(record.clone()));
    }
    assert_eq!(
        proof.record_for("member/Cargo.toml"),
        Some(relocated[0].1.clone())
    );
    assert!(proof.base_records(&"b".repeat(40)).is_some());
    assert!(proof.base_records(&"d".repeat(40)).is_none());
    assert!(!proof.applies_to(old.path()));
}

#[test]
fn source_and_existing_docs_bytes_preserve_metadata_identity() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    let before = resolution_inputs_digest(root.path(), &files, &records).expect("before");
    std::fs::write(root.path().join("README.md"), "updated docs").expect("docs");
    std::fs::write(
        root.path().join("member/src/lib.rs"),
        "pub fn changed() {}\n",
    )
    .expect("source");
    assert_eq!(
        before,
        resolution_inputs_digest(root.path(), &files, &records).expect("after")
    );
}

#[test]
fn lock_manifest_ignored_target_and_config_changes_invalidate() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    let original = resolution_inputs_digest(root.path(), &files, &records).expect("original");
    for path in [
        "Cargo.lock",
        "member/Cargo.toml",
        ".cargo/config.toml",
        "member/.cargo/config",
    ] {
        let path = root.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        let before = resolution_inputs_digest(root.path(), &files, &records).expect("before");
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        std::fs::write(path, format!("{existing}\n# changed\n")).expect("change");
        assert_ne!(
            before,
            resolution_inputs_digest(root.path(), &files, &records).expect("after")
        );
    }
    let before = resolution_inputs_digest(root.path(), &files, &records).expect("before target");
    std::fs::create_dir_all(root.path().join("member/src/bin")).expect("bins");
    std::fs::write(root.path().join("member/src/bin/new.rs"), "fn main() {}\n")
        .expect("ignored target");
    assert_ne!(
        before,
        resolution_inputs_digest(root.path(), &files, &records).expect("target")
    );
    assert_ne!(
        original,
        resolution_inputs_digest(root.path(), &files, &records).expect("changed")
    );
}

#[test]
fn payload_substitution_duplicate_keys_and_missing_fields_reject() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    let identity =
        identity(resolution_inputs_digest(root.path(), &files, &records).expect("digest"));
    let payload = build_payload(root.path(), identity.clone(), &records).expect("payload");
    let authority = RemoteAnalysisAuthority::fixture(identity, &payload);
    let replaced = payload.replace("proc-macro", "lib");
    assert!(parse_authenticated(root.path(), &files, &replaced, &authority).is_err());
    for invalid in ["{\"schema\":1,\"schema\":1}", "{}"] {
        assert!(super::payload_identity(invalid).is_err());
    }
}

#[test]
fn inferred_target_node_kind_change_invalidates() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    let before = resolution_inputs_digest(root.path(), &files, &records).expect("before");
    let target = root.path().join("member/src/lib.rs");
    std::fs::remove_file(&target).expect("remove inferred library");
    std::fs::create_dir(&target).expect("replace with directory");
    assert_ne!(
        before,
        resolution_inputs_digest(root.path(), &files, &records).expect("changed kind")
    );
}

#[test]
fn hidden_workspace_glob_manifests_and_empty_directories_invalidate() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers=['member','members/*']\n",
    )
    .expect("glob manifest");
    std::fs::create_dir_all(root.path().join("members/existing")).expect("existing member");
    std::fs::write(
        root.path().join("members/existing/Cargo.toml"),
        "[package]\nname='existing'\nversion='1.0.0'\n",
    )
    .expect("existing member manifest");
    let before =
        resolution_inputs_digest(root.path(), &files, &records).expect("before hidden member");
    std::fs::create_dir(root.path().join("members/hidden")).expect("hidden member directory");
    let directory =
        resolution_inputs_digest(root.path(), &files, &records).expect("hidden directory");
    assert_ne!(before, directory);
    std::fs::write(
        root.path().join("members/hidden/Cargo.toml"),
        "[package]\nname='hidden'\nversion='1.0.0'\n",
    )
    .expect("hidden manifest");
    assert_ne!(
        directory,
        resolution_inputs_digest(root.path(), &files, &records).expect("hidden manifest identity")
    );
}

#[test]
fn unqualified_cargo_source_commit_refuses_reuse_payload() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    let mut identity =
        identity(resolution_inputs_digest(root.path(), &files, &records).expect("digest"));
    identity.cargo_identity = "cargo 1.98.1 (012345678 2026-09-01)".to_owned();
    assert!(build_payload(root.path(), identity, &records).is_err());
}

#[test]
fn malformed_authenticated_source_branch_refuses_payload() {
    let root = tempfile::tempdir().expect("root");
    let (files, records) = fixture(root.path());
    let valid = identity(resolution_inputs_digest(root.path(), &files, &records).expect("digest"));
    let payload = build_payload(root.path(), valid.clone(), &records).expect("valid payload");
    for branch in ["", "main;echo", "main\nnext", "../main", "feature//invalid"] {
        let mut invalid = valid.clone();
        invalid.source.branch = branch.to_owned();
        assert!(
            build_payload(root.path(), invalid.clone(), &records).is_err(),
            "accepted {branch:?}"
        );
        let mut malformed: serde_json::Value =
            serde_json::from_str(&payload).expect("payload value");
        malformed["identity"]["source"]["branch"] = serde_json::json!(branch);
        let malformed = malformed.to_string();
        let authority = RemoteAnalysisAuthority::fixture(invalid, &malformed);
        assert!(parse_authenticated(root.path(), &files, &malformed, &authority).is_err());
    }
}
