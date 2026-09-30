//! Provenance validation tests (P04 matrix unit cases).
//!
//! Declared via `#[path]` from `provenance_check.rs` under `cfg(test)`.

use super::*;

/// Valid manifest plus matching expectations over `base`.
fn manifest_and_expected(base: &str) -> (BaselineManifest, ProvenanceExpectations) {
    let digest = digest_b3(b"d");
    let manifest = BaselineManifest {
        schema: 1,
        repository_id: digest.clone(),
        source_commit: base.to_owned(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/velnor.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: 9,
        artifact_name: baseline_artifact_name(base, &digest).expect("name"),
        tasks: Vec::new(),
        expires_at_unix: None,
    };
    let expected = ProvenanceExpectations {
        base: base.to_owned(),
        branch: "testmain".to_owned(),
        workflow_path: ".github/workflows/velnor.yml".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        repository_id: None,
    };
    (manifest, expected)
}

#[test]
fn every_wrong_dimension_fails_validation() {
    let base = "a".repeat(40);
    let (manifest, expected) = manifest_and_expected(&base);
    assert!(validate_provenance(&manifest, &digest_b3(b"m"), &expected).is_ok());
    let check = |mutate: &dyn Fn(&mut BaselineManifest), reason: &str| {
        let (mut manifest, expected) = manifest_and_expected(&base);
        mutate(&mut manifest);
        assert_eq!(
            validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err(reason),
            reason.to_owned()
        );
    };
    check(&|m| m.schema = 2, "stale_schema");
    check(&|m| m.source_commit = "b".repeat(40), "wrong_commit");
    check(&|m| m.ref_ = "refs/heads/other".to_owned(), "wrong_ref");
    check(&|m| m.event = "pull_request".to_owned(), "untrusted_proof");
    check(&|m| m.run_id = 0, "bad_proof_identity");
    check(
        &|m| m.generator_version = "9.9.9".to_owned(),
        "generator_mismatch",
    );
    check(
        &|m| m.generator_sha256 = "0".repeat(64),
        "generator_unverifiable",
    );
    check(
        &|m| m.workflow_ref = "o/r/other.yml@refs/heads/testmain".to_owned(),
        "wrong_workflow",
    );
    check(
        &|m| m.artifact_name = "forged".to_owned(),
        "artifact_mismatch",
    );
}

#[test]
fn repository_anchor_and_origins_normalize() {
    assert_eq!(
        normalize_origin_url("https://github.com/O/R.git"),
        Some("github.com/o/r".to_owned())
    );
    assert_eq!(
        normalize_origin_url("git@github.com:O/R.git"),
        Some("github.com/o/r".to_owned())
    );
    assert!(normalize_origin_url("not-a-url").is_none());
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(repository_anchor_from_origin(tmp.path()).is_none());
    std::fs::create_dir(tmp.path().join(".git")).expect("git");
    std::fs::write(
        tmp.path().join(".git/config"),
        "[remote \"origin\"]\n\turl = https://github.com/o/r.git\n",
    )
    .expect("config");
    let anchor = repository_anchor_from_origin(tmp.path()).expect("anchor");
    assert_eq!(anchor, digest_b3("github.com/o/r".as_bytes()));
    let base = "a".repeat(40);
    let (mut manifest, mut expected) = manifest_and_expected(&base);
    expected.repository_id = Some(anchor.clone());
    manifest.repository_id = anchor;
    assert!(validate_provenance(&manifest, &digest_b3(b"m"), &expected).is_ok());
    manifest.repository_id = digest_b3(b"other");
    assert_eq!(
        validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err("repo"),
        "wrong_repository".to_owned()
    );
}
