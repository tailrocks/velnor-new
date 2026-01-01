//! Reuse-stage tests.
//!
//! Declared via `#[path]` from `reuse_stages.rs` under `cfg(test)`.

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::digest_b3;

/// Bound task for every identity fixture.
const TASK_ID: &str = "stack/rust/root/clippy/default";

/// Complete descriptor over one source, output, env entry, and tool.
fn descriptor() -> CachedTaskDescriptor {
    CachedTaskDescriptor {
        task_name: "clippy".to_owned(),
        sources: vec!["Cargo.toml".to_owned()],
        outputs: vec!["out/report.json".to_owned()],
        command_inputs: Vec::new(),
        env: BTreeMap::from([("RUSTFLAGS".to_owned(), "--deny warnings".to_owned())]),
        tools: vec!["rust@1.98.1".to_owned()],
        dep_keys: Vec::new(),
    }
}

/// Expected identity plus matching restore metadata and live digest.
fn identity() -> (ExpectedReuseIdentity, ObservedRestoreMeta, String) {
    let key = digest_b3(b"key");
    let compat = digest_b3(b"compat");
    let inputs = digest_b3(b"inputs");
    (
        ExpectedReuseIdentity {
            cache_key: key.clone(),
            compatibility_id: compat.clone(),
            owner_scope: "trusted".to_owned(),
            input_digest: inputs.clone(),
        },
        ObservedRestoreMeta {
            task_id: TASK_ID.to_owned(),
            key,
            compat,
            owner: "trusted".to_owned(),
        },
        inputs,
    )
}

/// Declared outputs plus matching byte-verified observations.
fn outputs() -> (Vec<String>, Vec<ObservedOutput>) {
    let bytes = b"report-bytes".to_vec();
    let digest = digest_b3(&bytes);
    (
        vec!["out/report.json".to_owned()],
        vec![("out/report.json".to_owned(), bytes, digest)],
    )
}

#[test]
fn full_restore_verifies_and_stages_stay_separate() {
    let (declared, observed) = outputs();
    let (expected, restore, live) = identity();
    assert!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .is_ok()
    );
    assert_eq!(
        verify_reused_pipeline(&declared, &[], None, None, None, TASK_ID, None)
            .expect_err("presence"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
    assert_eq!(
        verify_reused_pipeline(&[], &observed, None, None, None, TASK_ID, None)
            .expect_err("eligibility"),
        MissReason::TASK_NOT_ELIGIBLE
    );
    let mut bare = descriptor();
    bare.outputs.clear();
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&bare),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("descriptor"),
        MissReason::TASK_NOT_ELIGIBLE
    );
    assert_eq!(
        verify_reused_pipeline(&declared, &observed, None, None, None, TASK_ID, None)
            .expect_err("trust gate"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
}

#[test]
fn identity_mismatches_fail_with_precise_reasons() {
    let (declared, observed) = outputs();
    let (expected, mut restore, live) = identity();
    restore.key = digest_b3(b"other");
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("key"),
        MissReason::INPUT_DIGEST_MISMATCH
    );
    let (_, mut restore, _) = identity();
    restore.compat = digest_b3(b"other");
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("compat"),
        MissReason::COMPATIBILITY_MISMATCH
    );
    let (_, mut restore, _) = identity();
    restore.owner = "pr".to_owned();
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("owner"),
        MissReason::TRUST_SCOPE_MISMATCH
    );
    let (_, restore, _) = identity();
    let mut tampered = observed.clone();
    tampered[0].1 = b"forged".to_vec();
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &tampered,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("bytes"),
        MissReason::CACHE_CORRUPT
    );
}

#[test]
fn missing_and_malformed_outputs_fail() {
    let (declared, _) = outputs();
    let (expected, restore, live) = identity();
    let run = |observed: &[ObservedOutput]| {
        verify_reused_pipeline(
            &declared,
            observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("output")
    };
    assert_eq!(
        run(&[("out/report.json".to_owned(), Vec::new(), digest_b3(b"x"))]),
        MissReason::TASK_RESULT_INCOMPLETE
    );
    assert_eq!(
        run(&[(
            "out/report.json".to_owned(),
            b"report-bytes".to_vec(),
            "bogus".to_owned()
        )]),
        MissReason::CACHE_CORRUPT
    );
    let other = b"report-bytes".to_vec();
    let other_digest = digest_b3(&other);
    assert_eq!(
        run(&[("out/other.json".to_owned(), other, other_digest)]),
        MissReason::TASK_RESULT_INCOMPLETE
    );
    assert_eq!(run(&[]), MissReason::TASK_RESULT_INCOMPLETE);
}

/// P04 zero-byte rule at the orchestrator layer: an empty observation
/// is incomplete even when its digest verifies, matching the mise layer.
#[test]
fn zero_byte_outputs_fail_orchestrator_layer_as_incomplete() {
    let (declared, _) = outputs();
    let (expected, restore, live) = identity();
    let empty = vec![("out/report.json".to_owned(), Vec::new(), digest_b3(b""))];
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &empty,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("zero byte"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
}

/// Task binding: restore evidence for another task is no entry for this
/// one, even when every digest matches.
#[test]
fn restore_for_another_task_is_no_entry() {
    let (declared, observed) = outputs();
    let (expected, mut restore, live) = identity();
    restore.task_id = "stack/rust/root/other/default".to_owned();
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&live),
        )
        .expect_err("task"),
        MissReason::NO_ENTRY
    );
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            "",
            Some(&live),
        )
        .expect_err("empty task"),
        MissReason::NO_ENTRY
    );
}

/// Same-path-changed-source: the live input digest flips while every
/// other stage passes, so the reuse rejects as a digest mismatch and a
/// missing live digest fails closed as incomplete.
#[test]
fn changed_live_inputs_reject_reuse() {
    let (declared, observed) = outputs();
    let (expected, restore, _) = identity();
    let changed = digest_b3(b"edited-sources");
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some(&changed),
        )
        .expect_err("changed sources"),
        MissReason::INPUT_DIGEST_MISMATCH
    );
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            Some("bogus"),
        )
        .expect_err("malformed live"),
        MissReason::INPUT_DIGEST_MISMATCH
    );
    assert_eq!(
        verify_reused_pipeline(
            &declared,
            &observed,
            Some(&descriptor()),
            Some(&expected),
            Some(&restore),
            TASK_ID,
            None,
        )
        .expect_err("missing live"),
        MissReason::TASK_RESULT_INCOMPLETE
    );
}
