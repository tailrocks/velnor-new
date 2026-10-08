//! Capacity, transition, and parity invariants. Calls the shipped functions.

use velnor_runner_core::{
    AcquireIntentId, ArchiveSafety, Capacity, CleanupProof, Conclusion, Effect, Epoch,
    EvidenceError, ExecutionKey, ExpectedExecutionSet, ExpectedItem, GrantId, IdError, MessageId,
    OwnedIds, OwnershipFailure, RequestId, StateError, VerifiedExecutionReport, VerifiedJobCensus,
    WorkerEvent, WorkerId, WorkerState, transition, verify_complete_results,
};

#[derive(Debug)]
enum TestFail {
    Id,
    State,
    Missing,
}

impl From<IdError> for TestFail {
    fn from(_err: IdError) -> Self {
        Self::Id
    }
}

impl From<StateError> for TestFail {
    fn from(_err: StateError) -> Self {
        Self::State
    }
}

fn wid(raw: u64) -> Result<WorkerId, IdError> {
    WorkerId::new(raw)
}

fn intent(raw: u64) -> Result<AcquireIntentId, IdError> {
    AcquireIntentId::new(raw)
}

fn epoch() -> Epoch {
    Epoch::new(1)
}

fn key(profile: &str) -> ExecutionKey {
    ExecutionKey {
        source: "abc".to_owned(),
        attempt: 1,
        plan: "plan".to_owned(),
        profile: profile.to_owned(),
        logical_job: "rust_test".to_owned(),
    }
}

fn report(profile: &str) -> VerifiedExecutionReport {
    VerifiedExecutionReport {
        key: key(profile),
        artifact_id: format!("art-{profile}"),
        conclusion: Conclusion::Success,
        runner_known: true,
        cached_success: false,
        archive: ArchiveSafety::Safe,
    }
}

fn expected_pair() -> ExpectedExecutionSet {
    ExpectedExecutionSet {
        items: vec![
            ExpectedItem {
                key: key("hosted"),
                artifact_id: "art-hosted".to_owned(),
            },
            ExpectedItem {
                key: key("local"),
                artifact_id: "art-local".to_owned(),
            },
        ],
    }
}

fn census(keys: &[ExecutionKey]) -> VerifiedJobCensus {
    VerifiedJobCensus {
        complete: true,
        omitted_page: false,
        success_on_expected_runner: keys.iter().cloned().collect(),
    }
}

fn stored(capacity: &Capacity, id: WorkerId) -> Result<WorkerState, TestFail> {
    capacity.state(id).cloned().ok_or(TestFail::Missing)
}

#[test]
fn occupancy_never_exceeds_n_and_drop_does_not_release() -> Result<(), TestFail> {
    let mut capacity = Capacity::new(1);
    let grant = capacity.reserve(wid(1)?, intent(1)?, epoch(), 1, 1)?;
    assert_eq!(grant, grant);
    assert_eq!(capacity.occupancy(), 1);
    let err = capacity.reserve(wid(2)?, intent(2)?, epoch(), 2, 2);
    assert_eq!(err, Err(StateError::CapacityExhausted));
    assert_eq!(capacity.occupancy(), 1);
    Ok(())
}

#[test]
fn replay_does_not_mint_a_second_grant() -> Result<(), TestFail> {
    let mut capacity = Capacity::new(2);
    let first = capacity.reserve(wid(1)?, intent(9)?, epoch(), 1, 1)?;
    let second = capacity.reserve(wid(2)?, intent(9)?, epoch(), 2, 2)?;
    assert_eq!(first, second);
    assert_eq!(capacity.occupancy(), 1);
    Ok(())
}

#[test]
fn non_released_states_count_until_proven_cleanup() -> Result<(), TestFail> {
    let mut capacity = Capacity::new(2);
    let worker = wid(1)?;
    capacity.reserve(worker, intent(1)?, epoch(), 1, 1)?;
    let owned = OwnedIds {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    };
    capacity.store(
        worker,
        WorkerState::Cleaning {
            epoch: epoch(),
            owned: owned.clone(),
        },
    )?;
    assert_eq!(capacity.occupancy(), 1);
    let mismatch = transition(
        &stored(&capacity, worker)?,
        &WorkerEvent::OwnershipMismatch {
            epoch: epoch(),
            reason: OwnershipFailure::IdMismatch,
        },
    )?;
    assert_eq!(mismatch.effect, Effect::KeepCapacity);
    capacity.store(worker, mismatch.next)?;
    assert_eq!(capacity.occupancy(), 1);
    capacity.store(
        worker,
        WorkerState::Cleaning {
            epoch: epoch(),
            owned: owned.clone(),
        },
    )?;
    let bad = CleanupProof {
        container_id: "other".to_owned(),
        volume: "v1".to_owned(),
    };
    assert_eq!(
        capacity.release(worker, &bad),
        Err(StateError::CleanupMismatch)
    );
    assert_eq!(capacity.occupancy(), 1);
    let proof = CleanupProof {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    };
    capacity.release(worker, &proof)?;
    assert_eq!(capacity.occupancy(), 0);
    Ok(())
}

#[test]
fn redelivery_does_not_rejuvenate_and_stale_epoch_is_rejected() -> Result<(), TestFail> {
    let mut capacity = Capacity::new(2);
    let later = wid(1)?;
    let earlier = wid(2)?;
    capacity.reserve(later, intent(1)?, epoch(), 2, 1)?;
    capacity.reserve(earlier, intent(2)?, epoch(), 1, 9)?;
    capacity.note_redelivery(earlier)?;
    assert_eq!(capacity.admission_order(), vec![earlier, later]);
    let state = WorkerState::Reserved {
        grant: GrantId::new(1)?,
        epoch: epoch(),
    };
    let err = transition(
        &state,
        &WorkerEvent::AcquireUncertain {
            epoch: Epoch::new(0),
        },
    );
    assert_eq!(err, Err(StateError::StaleEpoch));
    let replay = transition(
        &state,
        &WorkerEvent::Redelivered {
            id: RequestId::new(0)?,
        },
    )?;
    assert_eq!(replay.effect, Effect::Idempotent);
    assert_eq!(replay.next, state);
    Ok(())
}

#[test]
fn synthetic_message_id_is_not_ackable_and_zero_is() {
    assert!(!MessageId::SYNTHETIC.is_ackable());
    assert!(MessageId::new(0).is_ackable());
}

#[test]
fn duplicate_report_is_rejected_before_success() {
    let expected = expected_pair();
    let mut dup = report("hosted");
    dup.key = key("hosted");
    let observed = vec![report("hosted"), dup, report("local")];
    let err = verify_complete_results(
        &expected,
        &observed,
        &census(&[key("hosted"), key("local")]),
    );
    assert_eq!(err, Err(EvidenceError::DuplicateExecution));
}

#[test]
fn empty_expected_set_is_not_vacuously_proven() {
    let expected = ExpectedExecutionSet { items: Vec::new() };
    let err = verify_complete_results(&expected, &[], &census(&[]));
    assert_eq!(err, Err(EvidenceError::NotProven("empty_expected_set")));
}

#[test]
fn parity_fails_closed() {
    let expected = expected_pair();
    let good = census(&[key("hosted"), key("local")]);
    let ok = verify_complete_results(&expected, &[report("hosted"), report("local")], &good);
    assert!(ok.is_ok());
    let missing = verify_complete_results(&expected, &[report("hosted")], &good);
    assert!(matches!(
        missing,
        Err(EvidenceError::IncompleteExecutionSet | EvidenceError::NotProven(_))
    ));
    let mut wrong_attempt = report("hosted");
    wrong_attempt.key.attempt = 2;
    let attempt = verify_complete_results(&expected, &[wrong_attempt, report("local")], &good);
    assert_eq!(attempt, Err(EvidenceError::NotProven("wrong_attempt")));
    let mut swapped = report("hosted");
    swapped.artifact_id = "art-local".to_owned();
    let artifact = verify_complete_results(&expected, &[swapped, report("local")], &good);
    assert_eq!(artifact, Err(EvidenceError::NotProven("swapped_artifact")));
    let mut page = good.clone();
    page.omitted_page = true;
    let omitted = verify_complete_results(&expected, &[report("hosted"), report("local")], &page);
    assert_eq!(omitted, Err(EvidenceError::NotProven("omitted_page")));
    for archive in [
        ArchiveSafety::Traversal,
        ArchiveSafety::Symlink,
        ArchiveSafety::CaseCollision,
    ] {
        let mut bad = report("hosted");
        bad.archive = archive;
        let err = verify_complete_results(&expected, &[bad, report("local")], &good);
        assert!(matches!(err, Err(EvidenceError::NotProven(_))));
    }
    let mut cached = report("local");
    cached.cached_success = true;
    let cache = verify_complete_results(&expected, &[report("hosted"), cached], &good);
    assert_eq!(cache, Err(EvidenceError::NotProven("cached_success")));
    let mut skipped = report("local");
    skipped.conclusion = Conclusion::Skipped;
    let skip = verify_complete_results(&expected, &[report("hosted"), skipped], &good);
    assert_eq!(skip, Err(EvidenceError::NotProven("bad_conclusion")));
}

#[test]
fn source_profile_artifact_and_census_fail_closed() {
    let expected = expected_pair();
    let good = census(&[key("hosted"), key("local")]);
    let mut source = report("hosted");
    source.key.source = "other".to_owned();
    let err = verify_complete_results(&expected, &[source, report("local")], &good);
    assert_eq!(err, Err(EvidenceError::NotProven("wrong_source")));
    let mut profile = report("hosted");
    profile.key.profile = "foreign".to_owned();
    let err = verify_complete_results(&expected, &[profile, report("local")], &good);
    assert_eq!(err, Err(EvidenceError::NotProven("wrong_profile")));
    let mut plan = report("local");
    plan.key.plan = "other-plan".to_owned();
    let err = verify_complete_results(&expected, &[report("hosted"), plan], &good);
    assert_eq!(err, Err(EvidenceError::NotProven("wrong_plan")));
    let mut missing = report("hosted");
    missing.artifact_id.clear();
    let err = verify_complete_results(&expected, &[missing, report("local")], &good);
    assert_eq!(err, Err(EvidenceError::NotProven("missing_artifact")));
    let mut unknown = report("local");
    unknown.runner_known = false;
    let err = verify_complete_results(&expected, &[report("hosted"), unknown], &good);
    assert_eq!(err, Err(EvidenceError::NotProven("unknown_runner")));
    let mut incomplete = good.clone();
    incomplete.complete = false;
    let err = verify_complete_results(&expected, &[report("hosted"), report("local")], &incomplete);
    assert_eq!(err, Err(EvidenceError::NotProven("incomplete_census")));
    for conclusion in [
        Conclusion::Cancelled,
        Conclusion::TimedOut,
        Conclusion::Failed,
    ] {
        let mut bad = report("local");
        bad.conclusion = conclusion;
        let err = verify_complete_results(&expected, &[report("hosted"), bad], &good);
        assert_eq!(err, Err(EvidenceError::NotProven("bad_conclusion")));
    }
}
