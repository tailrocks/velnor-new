//! Merge-time entry-validation tests (shared `validate_task_entry`).
//!
//! Declared via `#[path]` from `cover_revalidate.rs` under `cfg(test)`.
//! Merge mirrors plan-time per-task checks through the same predicate,
//! so a manifest the plan rejects on entry grounds can never pass at
//! merge. Builders live in `cover_revalidate_fixtures.rs`.

use super::cover_revalidate_fixtures::{NOW, manifest_for, plan_for, verdict};
use super::*;
use velnor_actions_contract::ManifestTaskProof;

/// Merge rejects an entry whose structured proof binds other inputs.
///
/// The proof itself validates; only its binding to this entry fails,
/// exactly the divergence plan-time `validate_task_entry` rejects.
#[test]
fn merge_rejects_mismatched_structured_proof() {
    let commit = "a".repeat(40);
    let mut manifest = manifest_for(&commit);
    let entry = manifest.tasks[0].clone();
    manifest.tasks[0].proof = Some(
        ManifestTaskProof::new(
            &entry.task_id,
            &entry.task_digest,
            &digest_b3(b"other-inputs"),
            &entry.task_digest,
            &entry.task_digest,
            &entry.task_digest,
            &entry.task_digest,
            "default",
            entry.proof_run_id,
        )
        .expect("proof"),
    );
    let plan = plan_for(&manifest, Some(&commit));
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(signals.planning_failed, "mismatched proof must fail");
    assert!(miss.contains("cache_corrupt"), "{miss:?}");
}

/// Merge rejects a covered advisory claim with no external-data freshness.
///
/// The plan-anchored match still passes (the planner accepts the
/// manifest, then reruns the advisory task instead of covering it);
/// the covered claim fails at the per-obligation advisory gate, which
/// mirrors plan-time `gate_guards`.
#[test]
fn merge_rejects_advisory_without_external_data() {
    let commit = "a".repeat(40);
    let mut manifest = manifest_for(&commit);
    manifest.tasks[0].task_id = "stack/rust/root/advisory/default".to_owned();
    assert!(manifest.tasks[0].external_data.is_none());
    let mut plan = plan_for(&manifest, Some(&commit));
    plan.obligations[0].task_id = "stack/rust/root/advisory/default".to_owned();
    assert!(manifest_provenance_matches_plan(&plan, &manifest, NOW));
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(signals.planning_failed, "advisory needs freshness");
    assert!(miss.contains("cache_corrupt"), "{miss:?}");
}

/// Merge rejects a numeric ID that is not the name fingerprint.
///
/// The manifest author read a service-assigned ID back after upload;
/// the fingerprint conjunct fails exactly like the derived name does.
#[test]
fn merge_rejects_underived_numeric_id() {
    let commit = "a".repeat(40);
    let mut manifest = manifest_for(&commit);
    manifest.artifact_id = manifest.artifact_id.wrapping_add(1).max(1);
    let plan = plan_for(&manifest, Some(&commit));
    assert!(!manifest_provenance_matches_plan(&plan, &manifest, NOW));
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(signals.planning_failed, "fingerprint mismatch must fail");
    assert!(miss.contains("cache_corrupt"), "{miss:?}");
}

/// Merge accepts a covered advisory claim with fresh external data.
#[test]
fn merge_accepts_advisory_with_fresh_external_data() {
    let commit = "a".repeat(40);
    let mut manifest = manifest_for(&commit);
    manifest.tasks[0].task_id = "stack/rust/root/advisory/default".to_owned();
    manifest.tasks[0].external_data = Some(crate::external_data::ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: digest_b3(b"db"),
        age_secs: 60,
    });
    let mut plan = plan_for(&manifest, Some(&commit));
    plan.obligations[0].task_id = "stack/rust/root/advisory/default".to_owned();
    assert!(manifest_provenance_matches_plan(&plan, &manifest, NOW));
    let (signals, miss) = verdict(&plan, Some(&manifest));
    assert!(!signals.planning_failed, "{miss:?}");
}
