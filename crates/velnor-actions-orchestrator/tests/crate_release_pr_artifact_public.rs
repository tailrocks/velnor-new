// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use velnor_actions_orchestrator::crate_release_pr_artifact::{
    PreparationArtifact, PreparationBinding, PreparedFile, verify_preparation_artifact,
};

fn digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}

fn candidate() -> (PreparationArtifact, BTreeSet<String>) {
    let bytes = b"[package]\nversion = \"0.1.1\"\n".to_vec();
    (
        PreparationArtifact {
            schema: 1,
            binding: PreparationBinding {
                repository: "owner/project".into(),
                workflow_sha: "a".repeat(40),
                source_sha: "b".repeat(40),
                run_id: 12,
                run_attempt: 1,
            },
            files: vec![PreparedFile {
                path: "Cargo.toml".into(),
                sha256: digest(&bytes),
                bytes,
            }],
        },
        BTreeSet::from(["Cargo.toml".into()]),
    )
}

#[test]
fn accepts_exact_reviewed_run_and_allowlisted_file() -> Result<(), Box<dyn std::error::Error>> {
    let (artifact, allowed) = candidate();
    let bytes = serde_json::to_vec(&artifact)?;
    assert_eq!(
        verify_preparation_artifact(&bytes, &digest(&bytes), &artifact.binding, &allowed)?,
        artifact
    );
    Ok(())
}

#[test]
fn rejects_changed_run_digest_content_and_paths() -> Result<(), Box<dyn std::error::Error>> {
    let (artifact, allowed) = candidate();
    let bytes = serde_json::to_vec(&artifact)?;
    let mut other_run = artifact.binding.clone();
    other_run.run_attempt += 1;
    assert!(verify_preparation_artifact(&bytes, &digest(&bytes), &other_run, &allowed).is_err());
    let mut other_source = artifact.binding.clone();
    other_source.source_sha = "c".repeat(40);
    assert!(verify_preparation_artifact(&bytes, &digest(&bytes), &other_source, &allowed).is_err());
    assert!(
        verify_preparation_artifact(&bytes, &"0".repeat(64), &artifact.binding, &allowed).is_err()
    );
    let mut changed = artifact.clone();
    changed.files[0].bytes.push(b'X');
    let changed_bytes = serde_json::to_vec(&changed)?;
    assert!(
        verify_preparation_artifact(
            &changed_bytes,
            &digest(&changed_bytes),
            &artifact.binding,
            &allowed
        )
        .is_err()
    );
    let mut escaped = artifact.clone();
    escaped.files[0].path = "../Cargo.toml".into();
    let escaped_bytes = serde_json::to_vec(&escaped)?;
    assert!(
        verify_preparation_artifact(
            &escaped_bytes,
            &digest(&escaped_bytes),
            &artifact.binding,
            &BTreeSet::from(["../Cargo.toml".into()])
        )
        .is_err()
    );
    let mut unexpected = artifact.clone();
    unexpected.files[0].path = "README.md".into();
    let unexpected_bytes = serde_json::to_vec(&unexpected)?;
    assert!(
        verify_preparation_artifact(
            &unexpected_bytes,
            &digest(&unexpected_bytes),
            &artifact.binding,
            &allowed
        )
        .is_err()
    );
    let mut duplicated = artifact.clone();
    duplicated.files.push(duplicated.files[0].clone());
    let duplicate_bytes = serde_json::to_vec(&duplicated)?;
    assert!(
        verify_preparation_artifact(
            &duplicate_bytes,
            &digest(&duplicate_bytes),
            &artifact.binding,
            &allowed
        )
        .is_err()
    );
    Ok(())
}
