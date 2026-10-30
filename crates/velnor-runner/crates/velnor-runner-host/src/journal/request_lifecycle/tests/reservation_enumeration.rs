//! Connection-scoped physical reservation reads across restarts.

use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{Scratch, binding, created_scope, event_source, record_acquired, runner_name};
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, CapacityLimits, Journal, PhysicalRunnerJitState, PhysicalRunnerOwnership,
    ReservationState,
};
use velnor_runner_github::RunnerReference;

#[tokio::test]
async fn same_nonce_reservations_are_scoped_and_survive_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: JOB_LIMIT.memory_bytes * 2,
        })
        .await?;
    let first_binding = binding("essential", 101)?;
    let second_binding = binding("chain", 202)?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let second = created_scope(&journal, &second_binding, 502).await?;
    let (first_identity, first_intent) = physical_intent(&first, 'a').await?;
    let (second_identity, second_intent) = physical_intent(&second, 'a').await?;

    for (scope, expected, intent_id) in [
        (&first, &first_identity, first_intent),
        (&second, &second_identity, second_intent),
    ] {
        let rows = scope.physical_runner_reservations().await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].intent_id, intent_id);
        assert_eq!(rows[0].reservation.state, ReservationState::Reserved);
        assert_eq!(rows[0].jit_state, PhysicalRunnerJitState::Absent);
        assert_eq!(
            rows[0].ownership,
            PhysicalRunnerOwnership::Known(expected.clone())
        );
    }
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = journal.connection_scope(&first_binding).await?;
    let second = journal.connection_scope(&second_binding).await?;
    for (scope, expected, intent_id) in [
        (&first, &first_identity, first_intent),
        (&second, &second_identity, second_intent),
    ] {
        let rows = scope.physical_runner_reservations().await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].intent_id, intent_id);
        assert_eq!(
            rows[0].ownership,
            PhysicalRunnerOwnership::Known(expected.clone())
        );
        assert_eq!(rows[0].requested_runner_name, None);
    }
    Ok(())
}

#[tokio::test]
async fn uncertain_jit_remains_charged_and_enumerated_after_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 104).await?;
    let (identity, intent_id) = physical_intent(&scope, 'b').await?;
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('b'))
        .await?
        .ok_or(HostError::Journal)?;
    scope.mark_runner_jit_uncertain(claim).await?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let rows = scope.physical_runner_reservations().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].intent_id, intent_id);
    assert_eq!(rows[0].ownership, PhysicalRunnerOwnership::Known(identity));
    assert_eq!(rows[0].jit_state, PhysicalRunnerJitState::Uncertain);
    assert_eq!(rows[0].reservation.state, ReservationState::Reserved);
    assert_eq!(rows[0].requested_runner_name, Some(runner_name('b')));
    Ok(())
}

#[tokio::test]
async fn inconsistent_binding_is_enumerated_as_unknown_and_keeps_charge() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (identity, _) = physical_intent(&scope, 'd').await?;
    let reservation_id = scope
        .reservation(&identity)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    drop(scope);
    drop(journal);

    let database_path = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(database_path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservations SET binding_digest = ?1 WHERE id = ?2",
            ("0".repeat(64), reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let rows = scope.physical_runner_reservations().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ownership, PhysicalRunnerOwnership::Unknown);
    assert_eq!(rows[0].reservation.state, ReservationState::Reserved);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn completed_runner_remains_enumerated_until_cleanup_proofs() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 105).await?;
    let (identity, intent_id) = physical_intent(&scope, 'e').await?;
    let claim = scope
        .claim_runner_jit(intent_id, &runner_name('e'))
        .await?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: 705,
        name: runner_name('e'),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(claim, &runner).await?;
    let start = scope
        .claim_physical_runner_start(intent_id, &format!("w{}", "e".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_physical_worker_ready(start, "runner-e", "dind-e")
        .await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 105 })
        .await?;
    let source = event_source(101, 1_105);
    scope
        .record_job_started(&request, 905, &source, &runner, Some("opaque-job"))
        .await?;
    scope
        .record_job_completed(
            &request,
            906,
            &source,
            Some("success"),
            Some(&runner),
            Some("opaque-job"),
        )
        .await?;

    let rows = scope.physical_runner_reservations().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].intent_id, intent_id);
    assert_eq!(rows[0].ownership, PhysicalRunnerOwnership::Known(identity));
    assert_eq!(rows[0].reservation.state, ReservationState::Reserved);
    assert_eq!(rows[0].jit_state, PhysicalRunnerJitState::Completed);
    assert_eq!(rows[0].completed_request_id, Some(105));
    assert!(rows[0].completion_cleanup_pending);
    assert!(!rows[0].intent_cleanup_proven);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}
