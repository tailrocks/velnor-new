//! Independent bounded invocations replace credential-bearing polling.
use super::{TapMergeOutcome, attempt_once, ready_gate_fixture};
use std::cell::Cell;

#[test]
fn a_later_fresh_invocation_merges_once_after_pending_review() {
    let (_, policy, ready) = ready_gate_fixture();
    let mut pending = ready.clone();
    pending.completion = None;
    let writes = Cell::new(0);
    let first = attempt_once(
        &policy,
        || Ok(Some(pending.clone())),
        || panic!("pending must not refresh final authority"),
        |_| {
            writes.set(writes.get() + 1);
            Ok(())
        },
    )
    .expect("pending review");
    assert_eq!(first, TapMergeOutcome::Pending);
    assert_eq!(writes.get(), 0);
    let observations = Cell::new(0);
    let second = attempt_once(
        &policy,
        || {
            observations.set(observations.get() + 1);
            Ok(Some(ready.clone()))
        },
        || Ok(()),
        |actual| {
            assert_eq!(actual.current_head_sha, ready.current_head_sha);
            writes.set(writes.get() + 1);
            Ok(())
        },
    )
    .expect("fresh completed review");
    assert_eq!(second, TapMergeOutcome::Merged);
    assert_eq!(observations.get(), 2);
    assert_eq!(writes.get(), 1);
}

#[test]
fn final_asynchronous_pending_never_claims_or_performs_merge() {
    let (_, policy, ready) = ready_gate_fixture();
    let observations = Cell::new(0);
    let result = attempt_once(
        &policy,
        || {
            observations.set(observations.get() + 1);
            Ok((observations.get() == 1).then(|| ready.clone()))
        },
        || Ok(()),
        |_| panic!("final pending must not merge"),
    )
    .expect("pending after final observation");
    assert_eq!(result, TapMergeOutcome::Pending);
    assert_eq!(observations.get(), 2);
}

#[test]
fn final_base_drift_and_terminal_failed_checks_block_without_writes() {
    let (_, policy, ready) = ready_gate_fixture();
    let observations = Cell::new(0);
    let result = attempt_once(
        &policy,
        || {
            observations.set(observations.get() + 1);
            let mut current = ready.clone();
            if observations.get() == 2 {
                current.validated_base_sha = "d".repeat(40);
                current.current_base_sha = current.validated_base_sha.clone();
            }
            Ok(Some(current))
        },
        || Ok(()),
        |_| panic!("base drift must not merge"),
    );
    assert!(result.is_err());
    let mut failed = ready;
    failed.checks[0].completed_successfully = false;
    assert!(
        attempt_once(
            &policy,
            || Ok(Some(failed.clone())),
            || panic!("failed checks must not refresh"),
            |_| panic!("failed checks must not merge"),
        )
        .is_err()
    );
}

#[test]
fn final_source_refresh_follows_complete_observation_and_failure_blocks_merge() {
    let (_, policy, ready) = ready_gate_fixture();
    let observations = Cell::new(0);
    let result = attempt_once(
        &policy,
        || {
            observations.set(observations.get() + 1);
            Ok(Some(ready.clone()))
        },
        || {
            assert_eq!(observations.get(), 2);
            Err(crate::internal::internal("latest_ci_became_pending"))
        },
        |_| panic!("failed final freshness must not merge"),
    );
    assert!(result.is_err());
}
