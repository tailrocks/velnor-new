//! Workflow-slug and forwarded-proof tests.
//!
//! Declared via `#[path]` from `provenance_check.rs` under `cfg(test)`;
//! builders stay local so the dimension-matrix file keeps its size gate.

use super::*;

use velnor_actions_orchestrator_merge::required_evidence::BaselineTaskEntry;

/// Valid manifest plus matching expectations over `base`.
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
        compatibility_id: digest,
        artifact_id:
            velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id(
                &name,
            ),
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

/// Task entry observed by run 7 with an originating proof run.
fn task_entry(digest: &str, proof_run_id: u64) -> BaselineTaskEntry {
    BaselineTaskEntry {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: digest.to_owned(),
        closure_digest: digest.to_owned(),
        proof_run_id,
        observed_run_id: 7,
        external_data: None,
        proof: None,
    }
}

/// The workflow slug must name the anchored repo and agree with the
/// manifest's own repository id; internal ref consistency is enforced
/// too, and an unanchored slug fails closed.
#[test]
fn workflow_slug_binds_anchor_and_manifest() {
    let base = "a".repeat(40);
    let run = |manifest: &BaselineManifest, expected: &ProvenanceExpectations| {
        validate_provenance(manifest, &digest_b3(b"m"), expected)
    };
    let (manifest, expected) = manifest_and_expected(&base);
    assert!(run(&manifest, &expected).is_ok());
    let mut forked = manifest.clone();
    forked.workflow_ref = "evil/fork/.github/workflows/ci.yml@refs/heads/testmain".to_owned();
    assert_eq!(
        run(&forked, &expected).expect_err("fork slug"),
        "wrong_repository".to_owned()
    );
    let mut upper = manifest.clone();
    upper.workflow_ref = "O/R/.github/workflows/ci.yml@refs/heads/testmain".to_owned();
    assert!(run(&upper, &expected).is_ok(), "slugs compare lowercase");
    let mut split = manifest.clone();
    let evil = digest_b3(b"evil");
    split.repository_id = evil.clone();
    let mut split_expected = expected.clone();
    split_expected.repository_id = Some(evil);
    assert_eq!(
        run(&split, &split_expected).expect_err("split binding"),
        "wrong_repository".to_owned()
    );
    let mut unanchored = expected.clone();
    unanchored.repository_slug = None;
    assert_eq!(
        run(&manifest, &unanchored).expect_err("unanchored slug"),
        "repository_unanchored".to_owned()
    );
}

/// Forwarded proof runs fail closed: the manifest cannot prove the
/// originating run succeeded, so validation refuses instead of
/// warning-and-covering. Same-run proofs stay success-bound and pass.
#[test]
fn forwarded_proof_runs_fail_closed() {
    let base = "a".repeat(40);
    let digest = digest_b3(b"d");
    let run = |entry: &BaselineTaskEntry| {
        let (mut manifest, expected) = manifest_and_expected(&base);
        manifest.tasks = vec![entry.clone()];
        validate_provenance(&manifest, &digest_b3(b"m"), &expected)
    };
    let same = task_entry(&digest, 7);
    assert!(run(&same).is_ok(), "same-run proof validates");
    let forwarded = task_entry(&digest, 5);
    assert_eq!(
        run(&forwarded).expect_err("forwarded proof must fail"),
        "originating_run_unverified".to_owned()
    );
    // One forwarded entry poisons the whole manifest, even beside a
    // same-run entry.
    let (mut manifest, expected) = manifest_and_expected(&base);
    manifest.tasks = vec![same, forwarded];
    assert_eq!(
        validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err("mixed"),
        "originating_run_unverified".to_owned()
    );
}
