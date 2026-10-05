//! Expected-repository resolution tests.
//!
//! Declared via `#[path]` from `provenance_resolve.rs` under `cfg(test)`;
//! builders stay local so the dimension-matrix file keeps its size gate.

use super::super::provenance_check::{
    ProvenanceExpectations, baseline_artifact_name, validate_provenance,
};
use super::*;
use crate::merge::BaselineManifest;

/// Valid manifest plus matching expectations over `base`.
///
/// The repository id anchors `github.com/o/r` and the workflow slug
/// names `o/r`, so slug, anchor, and manifest binding all agree.
fn manifest_and_expected(base: &str) -> (BaselineManifest, ProvenanceExpectations) {
    let digest = digest_b3(b"d");
    let anchor = digest_b3("github.com/o/r".as_bytes());
    let name = baseline_artifact_name(base, &digest).expect("name");
    let manifest = BaselineManifest {
        schema: 2,
        repository_id: anchor.clone(),
        source_commit: base.to_owned(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: crate::cover_compat::baseline_artifact_numeric_id(&name),
        artifact_name: name,
        tasks: Vec::new(),
        expires_at_unix: None,
    };
    let expected = ProvenanceExpectations {
        base: base.to_owned(),
        branch: "testmain".to_owned(),
        workflow_path: ".github/workflows/ci.yml".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        repository_id: Some(anchor),
        repository_slug: Some("o/r".to_owned()),
        repository_conflict: false,
    };
    (manifest, expected)
}

/// Expected slug resolution: the env slug wins, the origin slug is the
/// unset-env fallback, disagreement conflicts, and malformed env
/// conflicts with no trusted slug (never a silent origin fallback).
#[test]
fn expected_repository_resolution_matrix() {
    let resolve = resolve_expected_repository;
    assert_eq!(
        resolve(Some("o/r"), None),
        ExpectedRepository {
            slug: Some("o/r".to_owned()),
            conflict: false,
        }
    );
    assert_eq!(
        resolve(None, None),
        ExpectedRepository {
            slug: None,
            conflict: false,
        }
    );
    assert_eq!(
        resolve(Some("o/r"), Some("O/R")),
        ExpectedRepository {
            slug: Some("o/r".to_owned()),
            conflict: false,
        }
    );
    assert_eq!(
        resolve(None, Some("o/r")),
        ExpectedRepository {
            slug: Some("o/r".to_owned()),
            conflict: false,
        }
    );
    assert_eq!(
        resolve(Some("o/r"), Some("evil/fork")),
        ExpectedRepository {
            slug: Some("evil/fork".to_owned()),
            conflict: true,
        }
    );
    assert_eq!(
        resolve(Some("o/r"), Some("not-a-slug")),
        ExpectedRepository {
            slug: None,
            conflict: true,
        }
    );
    assert_eq!(
        resolve(None, Some("not-a-slug")),
        ExpectedRepository {
            slug: None,
            conflict: true,
        }
    );
}

/// Malformed env plus an evil origin resolves to no trusted slug: the
/// origin must not launder through behind mangled env text.
#[test]
fn malformed_env_with_evil_origin_trusts_neither() {
    assert_eq!(
        resolve_expected_repository(Some("evil/fork"), Some("evil repo")),
        ExpectedRepository {
            slug: None,
            conflict: true,
        }
    );
}

/// A conflicted expectation fails closed even when the manifest would
/// otherwise validate: neither side is trusted once they disagree.
#[test]
fn conflicted_expectations_fail_closed() {
    let base = "a".repeat(40);
    let (manifest, mut expected) = manifest_and_expected(&base);
    assert!(validate_provenance(&manifest, &digest_b3(b"m"), &expected).is_ok());
    expected.repository_conflict = true;
    assert_eq!(
        validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err("conflict"),
        "wrong_repository".to_owned()
    );
}
