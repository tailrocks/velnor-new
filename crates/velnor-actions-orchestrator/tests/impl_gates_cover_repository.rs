//! Gate 5 cases: explicit repository capability pins baseline identity.
//!
//! Split from `impl_gates_cover` (size gate): hermetic coverage under
//! an explicit runner-owned repository slug.

use velnor_actions_contract::ObligationDecision;

use crate::impl_common::TestResult;
use crate::impl_gates_cover::{
    entries_for, manifest_for, plan_at_with_repository, plan_with_manifest,
};

/// An explicit repository slug agreeing with the origin covers.
///
/// The request capability `o/r` matches the fixture origin and the
/// manifest anchor, so coverage applies exactly like the local-run
/// fallback, under any ambient runner environment.
#[test]
fn explicit_matching_repository_covers() -> TestResult {
    let (repo, seed) = plan_with_manifest(None)?;
    let head = seed.head.clone();
    let manifest = manifest_for(&seed, &head, &entries_for(&seed));
    let plan = plan_at_with_repository(repo.path(), &head, &head, Some(manifest), Some("o/r"))?;
    assert!(!plan.obligations.is_empty());
    for ob in &plan.obligations {
        assert_eq!(
            ob.decision,
            ObligationDecision::CoveredByTrustedBaseline,
            "{ob:?}"
        );
    }
    assert_eq!(
        plan.baseline.status(),
        velnor_actions_contract::BaselineStatus::Used
    );
    Ok(())
}

/// An explicit repository slug disagreeing with the origin fails closed.
///
/// The request capability names a different repository than the
/// fixture checkout, so the manifest cannot validate: every
/// obligation executes and the baseline records its invalid reason.
/// This pins the fail-closed behavior CI once triggered by ambient
/// runner env leaking into classification.
#[test]
fn explicit_conflicting_repository_executes() -> TestResult {
    let (repo, seed) = plan_with_manifest(None)?;
    let head = seed.head.clone();
    let manifest = manifest_for(&seed, &head, &entries_for(&seed));
    let plan = plan_at_with_repository(
        repo.path(),
        &head,
        &head,
        Some(manifest),
        Some("someone/else"),
    )?;
    assert!(!plan.obligations.is_empty());
    for ob in &plan.obligations {
        assert_eq!(ob.decision, ObligationDecision::Execute, "{ob:?}");
        assert!(ob.baseline_proof.is_none(), "{ob:?}");
    }
    assert!(
        plan.baseline
            .reason()
            .is_some_and(|reason| reason.starts_with("baseline_invalid")),
        "{:?}",
        plan.baseline
    );
    Ok(())
}
