//! Ordinary coverage requires proof as strictly as changed-hint refinement.

use super::cover_identity_fixtures::*;
use super::*;
use velnor_actions_mise::ToolCatalog;

#[test]
fn missing_proof_refuses_ordinary_coverage_with_matching_closure() {
    let task = "stack/rust/root/clippy/default";
    let root = tempfile::tempdir().expect("root");
    seed_sources(root.path());
    let catalog = ToolCatalog::pinned();
    let discovery = discovery_with(&[task]);
    let closure = live_closure_digest(root.path(), &discovery, task, &catalog);
    let mut manifest = manifest_with(&[(task, &closure)]);
    let provenance = provenance_for(&manifest);
    manifest.tasks[0].proof = None;
    let mut plan = plan_with(&[task]);
    let covered = apply_coverage(
        &mut plan,
        &manifest,
        &provenance,
        &discovery,
        Some(&selection(&[])),
        &inputs(root.path(), &catalog),
    );
    assert_eq!(covered, 0);
    assert_eq!(plan.obligations[0].decision, ObligationDecision::Execute);
    assert!(
        plan.warnings
            .iter()
            .any(|warning| warning.contains("missing_task_proof"))
    );
}
