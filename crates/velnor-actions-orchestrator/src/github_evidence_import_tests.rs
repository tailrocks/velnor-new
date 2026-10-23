use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    GITHUB_EVIDENCE_SNAPSHOT_SCHEMA, GitHubEvidenceAcquisitionV1, GitHubEvidenceArtifactCaptureV1,
    GitHubEvidenceArtifactRefV1, GitHubEvidenceDocumentKindV1, GitHubEvidenceDocumentV1,
    GitHubEvidenceRepositoryV1, GitHubEvidenceSnapshotV1, canonical_json_bytes,
};

use super::files::{stage_bundle, verify_bundle};

fn fixture(root: &Path) -> GitHubEvidenceSnapshotV1 {
    fs::create_dir_all(root.join("documents")).expect("documents");
    fs::create_dir_all(root.join("archives")).expect("archives");
    let document = br#"{"id":1}"#;
    let archive = crate::api::github_evidence::test_support::archive();
    fs::write(root.join("documents/repository.json"), document).expect("document");
    fs::write(root.join("archives/7.zip"), &archive).expect("archive");
    let snapshot = GitHubEvidenceSnapshotV1 {
        schema: GITHUB_EVIDENCE_SNAPSHOT_SCHEMA,
        spec_sha256: "0".repeat(64),
        repository: GitHubEvidenceRepositoryV1 {
            id: 1,
            owner: "tailrocks".to_owned(),
            name: "velnor".to_owned(),
            default_branch: "main".to_owned(),
        },
        acquisition: GitHubEvidenceAcquisitionV1 {
            run_id: 2,
            run_attempt: 1,
            collector_ref: "refs/heads/main".to_owned(),
            collector_sha: "a".repeat(40),
            captured_at: "2026-10-23T00:00:00Z".to_owned(),
            window_start: "2026-10-22T00:00:00Z".to_owned(),
            window_end: "2026-10-23T00:00:00Z".to_owned(),
        },
        workflow_sources: Vec::new(),
        documents: vec![GitHubEvidenceDocumentV1 {
            kind: GitHubEvidenceDocumentKindV1::Repository,
            workflow_path: None,
            run_id: None,
            attempt: None,
            artifact_id: None,
            page: None,
            path: "documents/repository.json".to_owned(),
            size_bytes: document.len() as u64,
            sha256: digest(document),
        }],
        run_attempts: Vec::new(),
        artifacts: vec![GitHubEvidenceArtifactCaptureV1 {
            source: GitHubEvidenceArtifactRefV1 {
                suite_id: "producer".to_owned(),
                artifact_name: "report".to_owned(),
                workflow_path: ".github/workflows/producer.yml".to_owned(),
                job_id: "mise-task-producer".to_owned(),
                job_name: "Mise task producer".to_owned(),
            },
            artifact_id: 7,
            run_id: 3,
            name: "producer-report".to_owned(),
            path: "archives/7.zip".to_owned(),
            metadata_document: 0,
            size_bytes: archive.len() as u64,
            sha256: digest(&archive),
        }],
    };
    fs::write(
        root.join("index.json"),
        canonical_json_bytes(&snapshot).expect("canonical index"),
    )
    .expect("index");
    snapshot
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut output = String::with_capacity(hash.len() * 2);
    for byte in hash {
        write!(&mut output, "{byte:02x}").expect("format into String");
    }
    output
}

#[test]
fn import_verifies_and_stages_exact_original_files() {
    let temp = tempfile::tempdir().expect("temp");
    let bundle = temp.path().join("bundle");
    let workspace = temp.path().join("workspace");
    fs::create_dir(&bundle).expect("bundle");
    fs::create_dir(&workspace).expect("workspace");
    fs::create_dir(workspace.join(".velnor")).expect("config directory");
    let snapshot = fixture(&bundle);

    verify_bundle(&bundle, &snapshot).expect("valid source bundle");
    stage_bundle(&bundle, &workspace, &snapshot).expect("staging");
    let staged = workspace.join(".velnor/evidence");
    assert_eq!(
        fs::read(staged.join("index.json")).expect("index"),
        fs::read(bundle.join("index.json")).expect("source index")
    );
    assert_eq!(
        fs::read(staged.join("archives/7.zip")).expect("archive"),
        crate::api::github_evidence::test_support::archive()
    );
    verify_bundle(&staged, &snapshot).expect("valid staged bundle");
}

#[test]
fn import_rejects_missing_extra_and_changed_archive_files() {
    let temp = tempfile::tempdir().expect("temp");
    let bundle = temp.path().join("bundle");
    fs::create_dir(&bundle).expect("bundle");
    let snapshot = fixture(&bundle);

    fs::remove_file(bundle.join("archives/7.zip")).expect("remove archive");
    assert!(verify_bundle(&bundle, &snapshot).is_err());
    fs::write(
        bundle.join("archives/7.zip"),
        crate::api::github_evidence::test_support::archive(),
    )
    .expect("restore archive");
    fs::write(bundle.join("extra.txt"), b"extra").expect("extra");
    assert!(verify_bundle(&bundle, &snapshot).is_err());
    fs::remove_file(bundle.join("extra.txt")).expect("remove extra");
    fs::write(bundle.join("archives/7.zip"), b"PK\x03\x05").expect("change archive");
    assert!(verify_bundle(&bundle, &snapshot).is_err());
}

#[test]
fn import_rejects_pk_prefix_when_indexed_size_and_digest_match() {
    let temp = tempfile::tempdir().expect("temp");
    let bundle = temp.path().join("bundle");
    fs::create_dir(&bundle).expect("bundle");
    let mut snapshot = fixture(&bundle);
    let invalid = b"PK\x03\x04";
    fs::write(bundle.join("archives/7.zip"), invalid).expect("replace archive");
    snapshot.artifacts[0].size_bytes = invalid.len() as u64;
    snapshot.artifacts[0].sha256 = digest(invalid);
    fs::write(
        bundle.join("index.json"),
        canonical_json_bytes(&snapshot).expect("canonical index"),
    )
    .expect("replace index");
    assert!(verify_bundle(&bundle, &snapshot).is_err());
}
