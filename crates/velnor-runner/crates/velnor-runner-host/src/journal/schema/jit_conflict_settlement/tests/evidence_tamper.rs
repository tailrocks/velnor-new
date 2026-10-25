use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::request_lifecycle::tests::helpers::binding;
use velnor_runner_github::{
    Exchange, RunnerReference, RunnerResponseReceipt, SessionRequest, Transport, TransportFail,
    observe_runner_by_name,
};

use super::{LocalAbsenceFixture, local_absence_fixture, verified_empty_lookup};

#[path = "evidence_tamper/forged_progress.rs"]
mod forged_progress;

#[inline(never)]
fn prepare_forged_progress_case(
    nonce: char,
    set_id: i64,
    request_id: i64,
    observed: bool,
    phase_indices: &'static [i64],
) -> impl std::future::Future<
    Output = Result<
        (
            std::path::PathBuf,
            crate::journal::request_lifecycle::tests::helpers::Scratch,
        ),
        HostError,
    >,
> + Send {
    Box::pin(async move {
        let fixture = local_absence_fixture(nonce, set_id, request_id).await?;
        let fixture = if observed {
            record_remote_initial_absence(fixture).await?
        } else {
            fixture
        };
        reject_forged_progress_evidence(fixture, phase_indices).await
    })
}
#[inline(never)]
fn record_remote_initial_absence(
    fixture: LocalAbsenceFixture,
) -> impl std::future::Future<Output = Result<LocalAbsenceFixture, HostError>> + Send {
    Box::pin(async move {
        let receipt = verified_empty_lookup(&fixture.requested_name)?;
        fixture
            .scope
            .record_jit_conflict_remote_initial_absence(&fixture.recovery_claim, &receipt)
            .await?;
        Ok(fixture)
    })
}
async fn reject_forged_progress_evidence(
    fixture: LocalAbsenceFixture,
    phase_indices: &[i64],
) -> Result<
    (
        std::path::PathBuf,
        crate::journal::request_lifecycle::tests::helpers::Scratch,
    ),
    HostError,
> {
    let path = fixture.scratch.file();
    let reservation_id = fixture.physical.reservation_id;
    let claim_generation = fixture.recovery_claim.claim_generation();
    let LocalAbsenceFixture {
        scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    finish_reject_forged_progress_evidence(
        scratch,
        path,
        reservation_id,
        claim_generation,
        reopened,
        phase_indices,
    )
    .await
}
#[inline(never)]
fn finish_reject_forged_progress_evidence(
    scratch: crate::journal::request_lifecycle::tests::helpers::Scratch,
    path: std::path::PathBuf,
    reservation_id: i64,
    claim_generation: i64,
    reopened: Journal,
    phase_indices: &[i64],
) -> impl std::future::Future<
    Output = Result<
        (
            std::path::PathBuf,
            crate::journal::request_lifecycle::tests::helpers::Scratch,
        ),
        HostError,
    >,
> + Send
+ '_ {
    Box::pin(async move {
        let connection = reopened.connection().await?;
        let update_guard = object_sql(
            &connection,
            "trigger",
            "physical_attempt_jit_conflict_progress_no_update",
        )
        .await?;
        connection
            .execute(
                "DROP TRIGGER physical_attempt_jit_conflict_progress_no_update",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let forged_digest = "0".repeat(64);
        for phase_index in phase_indices {
            assert_eq!(
            connection
                .execute(
                    "UPDATE physical_attempt_jit_conflict_recovery_progress SET evidence_sha256=?1 WHERE reservation_id=?2 AND claim_generation=?3 AND phase_index=?4",
                    (forged_digest.as_str(), reservation_id, claim_generation, *phase_index),
                )
                .await
                .map_err(|_| HostError::Journal)?,
            1
        );
        }
        connection
            .execute(&update_guard, ())
            .await
            .map_err(|_| HostError::Journal)?;
        drop(connection);
        drop(reopened);

        Ok((path, scratch))
    })
}

#[tokio::test]
async fn delete_tombstone_survives_reclaim_without_replay() -> Result<(), HostError> {
    Box::pin(delete_tombstone_survives_reclaim_without_replay_inner()).await
}

async fn delete_tombstone_survives_reclaim_without_replay_inner() -> Result<(), HostError> {
    let fixture = local_absence_fixture('1', 828, 1_128).await?;
    finish_delete_tombstone_survives_reclaim_without_replay(fixture).await
}

#[inline(never)]
fn finish_delete_tombstone_survives_reclaim_without_replay(
    fixture: LocalAbsenceFixture,
) -> impl std::future::Future<Output = Result<(), HostError>> + Send {
    Box::pin(async move {
        let runner = RunnerReference {
            id: 9_128,
            name: fixture.requested_name.clone(),
            runner_scale_set_id: 828,
        };
        let first_generation = fixture.recovery_claim.claim_generation();
        record_initial_target_and_delete(&fixture, &runner).await?;
        let retry = reclaim_after_delete_tombstone(fixture, first_generation).await?;
        assert!(
            retry
                .scope
                .resume_jit_conflict_local_absence(&retry.recovery_claim)
                .await?
        );
        assert!(
            retry
                .scope
                .jit_conflict_delete_started(&retry.recovery_claim)
                .await?
        );
        assert_eq!(
            retry
                .scope
                .jit_conflict_delete_target(&retry.recovery_claim)
                .await?,
            Some(runner.clone())
        );
        assert_eq!(remote_cleanup_flag(&retry).await?, 0);
        assert_replay_is_rejected(&retry, &runner).await?;
        assert_eq!(remote_cleanup_flag(&retry).await?, 1);

        let connection = retry.journal.connection().await?;
        assert_retry_history(
            &connection,
            retry.physical.reservation_id,
            retry.recovery_claim.claim_generation(),
        )
        .await?;
        super::super::super::validate_persisted_schema_v40(&connection).await?;
        Ok(())
    })
}

async fn record_initial_target_and_delete(
    fixture: &LocalAbsenceFixture,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let found = found_lookup(runner)?;
    fixture
        .scope
        .record_jit_conflict_remote_target(&fixture.recovery_claim, runner, &found)
        .await?;
    fixture
        .scope
        .record_jit_conflict_delete_started(&fixture.recovery_claim, runner)
        .await?;
    assert!(
        fixture
            .scope
            .jit_conflict_delete_started(&fixture.recovery_claim)
            .await?
    );
    assert_eq!(
        fixture
            .scope
            .jit_conflict_delete_target(&fixture.recovery_claim)
            .await?,
        Some(runner.clone())
    );
    Ok(())
}

async fn remote_cleanup_flag(fixture: &LocalAbsenceFixture) -> Result<i64, HostError> {
    let connection = fixture.journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT remote_cleanup_proven FROM capacity_reservations WHERE id=?1",
            [fixture.physical.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn reclaim_after_delete_tombstone(
    fixture: LocalAbsenceFixture,
    first_generation: i64,
) -> Result<LocalAbsenceFixture, HostError> {
    let path = fixture.scratch.file();
    let reservation_id = fixture.physical.reservation_id;
    let repository = binding("jit-v40-null-1", 101)?;
    super::fixtures::expire_recovery_claim(&fixture, first_generation).await?;
    let LocalAbsenceFixture {
        scratch,
        journal,
        scope,
        physical,
        requested_name,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&repository).await?;
    let candidate = scope
        .recoverable_jit_conflict_candidates()
        .await?
        .into_iter()
        .find(|candidate| candidate.reservation_id() == reservation_id)
        .ok_or(HostError::Journal)?;
    let recovery_claim = scope
        .claim_jit_conflict_candidate(&candidate, 180)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(recovery_claim.claim_generation(), first_generation + 1);
    Ok(LocalAbsenceFixture {
        scratch,
        journal,
        scope,
        recovery_claim,
        physical,
        requested_name,
    })
}

async fn assert_replay_is_rejected(
    fixture: &LocalAbsenceFixture,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let repeated_target = found_lookup(runner)?;
    assert!(
        fixture
            .scope
            .record_jit_conflict_remote_target(&fixture.recovery_claim, runner, &repeated_target)
            .await
            .is_err()
    );
    assert!(
        fixture
            .scope
            .record_jit_conflict_delete_started(&fixture.recovery_claim, runner)
            .await
            .is_err()
    );
    let fresh_absence = verified_empty_lookup(&fixture.requested_name)?;
    fixture
        .scope
        .record_jit_conflict_remote_absence(&fixture.recovery_claim, &fresh_absence)
        .await
}

async fn assert_retry_history(
    connection: &turso::Connection,
    reservation_id: i64,
    retry_generation: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT SUM(CASE WHEN phase_index=2 AND phase='remote_target' THEN 1 ELSE 0 END),SUM(CASE WHEN phase_index=3 AND phase='delete_started' THEN 1 ELSE 0 END),SUM(CASE WHEN phase_index=4 AND phase='remote_absent' THEN 1 ELSE 0 END),SUM(CASE WHEN claim_generation=?2 AND phase_index=2 THEN 1 ELSE 0 END),SUM(CASE WHEN claim_generation=?2 AND phase_index=3 THEN 1 ELSE 0 END) FROM physical_attempt_jit_conflict_recovery_progress WHERE reservation_id=?1",
            (reservation_id, retry_generation),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    for (column, expected) in [(0, 1), (1, 1), (2, 1), (3, 0), (4, 0)] {
        assert_eq!(
            row.get::<i64>(column).map_err(|_| HostError::Journal)?,
            expected
        );
    }
    Ok(())
}

fn found_lookup(runner: &RunnerReference) -> Result<RunnerResponseReceipt, HostError> {
    let mut transport = FoundLookupTransport(runner.clone());
    let (found, receipt) = observe_runner_by_name(&mut transport, &runner.name, "fixture-token")
        .map_err(|_| HostError::Journal)?;
    if found.as_ref() != Some(runner) {
        return Err(HostError::Journal);
    }
    Ok(receipt)
}

struct FoundLookupTransport(RunnerReference);

impl Transport for FoundLookupTransport {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        let body = format!(
            r#"{{"count":1,"value":[{{"id":{},"name":"{}","runnerScaleSetId":{}}}]}}"#,
            self.0.id, self.0.name, self.0.runner_scale_set_id
        );
        Ok(Exchange {
            status: 200,
            body: body.into_bytes(),
        })
    }
}

async fn object_sql(
    connection: &turso::Connection,
    kind: &str,
    name: &str,
) -> Result<String, HostError> {
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type=?1 AND name=?2 AND sql IS NOT NULL",
            (kind, name),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}
