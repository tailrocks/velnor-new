//! T25 tofu benchmark addition: the corrupt-cache matrix case.
//!
//! The T24 matrix (cold, warm, docs, root, module, lock, mixed, fork)
//! has no corrupt-cache case; the contract matrix requires one. No
//! runner executes locally, so this case measures the honest local
//! analogue: planning a lockful fixture while the provider restore
//! verifier rejects a tampered entry with `cache_corrupt` (cold
//! recovery = discard plus execute). The wall is plan-dominated;
//! the restore verdict proves the corrupt entry never verifies.

use velnor_actions_contract::digest_b3;
use velnor_actions_mise::restore_evidence::{RestoreObservation, verify_provider_restore};

use crate::impl_perf_p13::perf_harness_p13::{BenchSample, bench_line, timed, timed_rss};
use crate::impl_tofu_t24_gates::tofu_perf_fixtures_t24::{
    commit_two_tofu, index_baseline_ms, plan_at_event, tofu_repo_with_lock,
};
use crate::support::TestResult;

/// Tampered provider bytes against the recorded digest.
fn corrupt_observation() -> RestoreObservation {
    RestoreObservation {
        entry_path: "tofu-providers/a.tar.gz".to_owned(),
        entry_bytes: b"tampered-provider-bytes".to_vec(),
        expected_digest: digest_b3(b"provider-bytes"),
        expected_compat: digest_b3(b"compat"),
        observed_compat: digest_b3(b"compat"),
        expected_owner: "trust-scope".to_owned(),
        observed_owner: "trust-scope".to_owned(),
        expected_inputs: digest_b3(b"inputs"),
        observed_inputs: digest_b3(b"inputs"),
    }
}

/// Corrupt provider cache: restore rejects, the lockful root plans.
#[test]
fn bench_tofu_corrupt_cache_rejects_and_plans() -> TestResult {
    let (repo, setup_ms) = timed(tofu_repo_with_lock);
    let repo = repo?;
    let root = repo.path();
    let (outcome, plan_ms, rss_kb) = timed_rss(|| {
        let touched = commit_two_tofu(root, "stacks/a/.terraform.lock.hcl", "# bump\n");
        touched.and_then(|(base, head)| plan_at_event(root, &base, &head, "pull_request"))
    });
    let (plan, _) = outcome?;
    assert_eq!(plan.obligations.len(), 3, "one lockful root");
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.reason == "affected_by_change"),
        "lock change selects the root"
    );
    assert_eq!(
        verify_provider_restore(&corrupt_observation()),
        Err("cache_corrupt"),
        "tampered provider bytes never verify"
    );
    let mut clean = corrupt_observation();
    clean.entry_bytes = b"provider-bytes".to_vec();
    assert_eq!(
        verify_provider_restore(&clean),
        Ok(()),
        "recorded bytes verify as control"
    );
    let metadata_ms = index_baseline_ms(root)?;
    bench_line(&BenchSample {
        case: "tofu-corrupt-cache",
        crates: 1,
        setup_ms,
        plan_ms,
        metadata_ms,
        rss_kb,
        plan: &plan,
        note: "restore=cache_corrupt",
    });
    Ok(())
}
