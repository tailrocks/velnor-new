//! Connection-scoped physical reservation reads across restarts.

use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{
    Scratch, binding, created_scope, event_source, finish_initialized_listener_delivery,
    provision_acquired_request_with_initialization, runner_name,
};
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, CapacityLimits, Journal, PhysicalRunnerJitState, PhysicalRunnerOwnership,
    ReservationState,
};
use velnor_runner_github::RunnerReference;

async fn reserve_only(
    scope: &crate::journal::ConnectionJournal,
    nonce: char,
    sample_nonce: char,
) -> Result<(crate::journal::AssignmentIdentity, i64), HostError> {
    use crate::journal::{
        ReservationRequest, ReservationResult, fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    let sample = format!("{:032x}", sample_nonce as u32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, super::helpers::TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: super::helpers::TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    Ok((assignment, reservation_id))
}

#[tokio::test]
async fn reservations_are_scoped_and_survive_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: JOB_LIMIT.memory_bytes * 4,
        })
        .await?;
    let first_binding = binding("essential", 101)?;
    let second_binding = binding("chain", 202)?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let second = created_scope(&journal, &second_binding, 502).await?;
    let (first_identity, first_reservation) = reserve_only(&first, 'a', 'a').await?;
    let (second_identity, second_reservation) = reserve_only(&second, 'b', 'b').await?;
    assert_ne!(first_identity, second_identity);

    for (scope, expected, reservation_id) in [
        (&first, &first_identity, first_reservation),
        (&second, &second_identity, second_reservation),
    ] {
        let rows = scope.physical_runner_reservations().await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reservation_id, reservation_id);
        assert!(rows[0].intent_id > 0);
        assert_eq!(rows[0].reservation.state, ReservationState::Reserved);
        assert_eq!(rows[0].jit_state, PhysicalRunnerJitState::Absent);
        assert_eq!(
            rows[0].ownership,
            PhysicalRunnerOwnership::Known(expected.clone())
        );
    }
    assert_eq!(journal.occupied_reservation_count().await?, 2);
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = journal.connection_scope(&first_binding).await?;
    let second = journal.connection_scope(&second_binding).await?;
    for (scope, expected, reservation_id) in [
        (&first, &first_identity, first_reservation),
        (&second, &second_identity, second_reservation),
    ] {
        let rows = scope.physical_runner_reservations().await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reservation_id, reservation_id);
        assert!(rows[0].intent_id > 0);
        assert_eq!(
            rows[0].ownership,
            PhysicalRunnerOwnership::Known(expected.clone())
        );
        assert_eq!(rows[0].requested_runner_name, None);
    }
    assert_eq!(journal.occupied_reservation_count().await?, 2);
    Ok(())
}

#[tokio::test]
async fn same_nonce_is_scoped_but_duplicate_global_resource_names_roll_back()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 2,
            cpu_millicores: JOB_LIMIT.cpu_millicores * 2,
            memory_bytes: JOB_LIMIT.memory_bytes * 4,
        })
        .await?;
    let first_binding = binding("essential", 101)?;
    let second_binding = binding("chain", 202)?;
    let first = created_scope(&journal, &first_binding, 501).await?;
    let second = created_scope(&journal, &second_binding, 502).await?;
    let (first_identity, first_reservation) = reserve_only(&first, 'a', 'a').await?;
    let second_identity = second
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "a".repeat(32),
        })
        .await?;
    assert_ne!(first_identity, second_identity);
    assert!(reserve_only(&second, 'a', 'b').await.is_err());

    let first_rows = first.physical_runner_reservations().await?;
    assert_eq!(first_rows.len(), 1);
    assert_eq!(first_rows[0].reservation_id, first_reservation);
    assert_eq!(
        first_rows[0].ownership,
        PhysicalRunnerOwnership::Known(first_identity.clone())
    );
    assert_eq!(second.physical_runner_reservations().await?, Vec::new());
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_intents WHERE connection_id = ?1 AND assignment_key = ?2",
            (second_identity.connection_id().as_str(), second_identity.storage_key()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(row.get::<i64>(0).map_err(|_| HostError::Journal)?, 0);
    drop(rows);
    drop(connection);
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let first = journal.connection_scope(&first_binding).await?;
    let second = journal.connection_scope(&second_binding).await?;
    let first_rows = first.physical_runner_reservations().await?;
    assert_eq!(first_rows.len(), 1);
    assert_eq!(first_rows[0].reservation_id, first_reservation);
    assert_eq!(
        first_rows[0].ownership,
        PhysicalRunnerOwnership::Known(first_identity)
    );
    assert_eq!(second.physical_runner_reservations().await?, Vec::new());
    assert_eq!(journal.occupied_reservation_count().await?, 1);
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
    let initialized = provision_acquired_request_with_initialization(&scope, 104, 'b', 'b').await?;
    let intent_id = initialized.intent_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    let claim = scope
        .claim_runner_jit_for_request(
            intent_id,
            initialized.reservation_id,
            104,
            &runner_name('b'),
            generation,
            &digest,
        )
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
    assert_eq!(rows[0].ownership, PhysicalRunnerOwnership::Unknown);
    assert_eq!(rows[0].jit_state, PhysicalRunnerJitState::Uncertain);
    assert_eq!(rows[0].reservation.state, ReservationState::Uncertain);
    assert!(!rows[0].reservation.resume_eligible);
    assert_eq!(rows[0].requested_runner_name, Some(runner_name('b')));
    assert!(!rows[0].remote_cleanup_proven());
    assert!(!rows[0].local_cleanup_proven());
    assert_eq!(journal.occupied_reservation_count().await?, 1);
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
    let connection = journal.connection().await?;
    connection
        .execute(
            "UPDATE capacity_reservations SET binding_digest = ?1 WHERE id = ?2",
            ("0".repeat(64), reservation_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);

    let scope = journal.connection_scope(&binding).await?;
    let rows = scope.physical_runner_reservations().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ownership, PhysicalRunnerOwnership::Unknown);
    assert_eq!(rows[0].reservation.state, ReservationState::Reserved);
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    drop(scope);
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn completed_runner_remains_enumerated_until_cleanup_proofs() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 105, 'e', 'e').await?;
    let identity = initialized.reservation_assignment.clone();
    let intent_id = initialized.intent_id;
    let runner = RunnerReference {
        id: 705,
        name: runner_name('e'),
        runner_scale_set_id: 501,
    };
    finish_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 105).await?;
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
    assert!(!rows[0].intent_cleanup_proven());
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}
