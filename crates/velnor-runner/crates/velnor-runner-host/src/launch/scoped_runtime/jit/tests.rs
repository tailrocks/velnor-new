use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    error::HostError,
    journal::{CapacityLimits, ReservationState},
    launch::Lane,
    reconcile::LaunchPhase,
    scale_set::EnsureError,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

#[derive(Clone)]
struct ConflictTransport(Arc<AtomicUsize>);

impl Transport for ConflictTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        assert!(request.path.ends_with("/generatejitconfig"));
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Exchange {
            status: 409,
            body: Vec::new(),
        })
    }
}

impl Lane for ConflictTransport {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
    fn use_github_api(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

#[tokio::test]
async fn jit_conflict_keeps_resources_charged_and_never_replays() -> Result<(), HostError> {
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, provision_physical_intent_with_initialization,
    };
    use crate::journal::{
        AssignmentKey, Journal, ReservationRequest, ReservationResult,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const SET_ID: i64 = 501;
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("jit-conflict-bound", 101)?;
    let scope = created_scope(&journal, &repository, SET_ID).await?;
    crate::journal::request_lifecycle::tests::helpers::record_acquired(&scope, 70).await?;
    let physical = provision_physical_intent_with_initialization(&scope, 'b', 'b').await?;
    let runner_name = format!("v{}", "b".repeat(32));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut transport = ConflictTransport(calls.clone());

    let result = super::request_and_bind(
        &mut transport,
        "test-admin-token",
        &scope,
        physical.intent_id,
        &runner_name,
        SET_ID,
    )
    .await;
    assert!(matches!(result, Err(EnsureError::Conflict)));

    let rows = scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(row.runner, None);
    assert_eq!(row.witness_request_id, Some(70));
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        scope
            .claim_listener_delivery(physical.reservation_id)
            .await?
            .is_none()
    );
    let competing_generation = "c".repeat(32);
    let competing_permit = StartPermit::test_fixture_with_engine_id(
        &competing_generation,
        crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID,
    );
    let competing_assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: competing_generation.clone(),
        })
        .await?;
    let competing_admission = fixture_storage_provisioning_admission(&competing_permit)?;
    let competing_docker_root = competing_permit.docker_root_digest().to_owned();
    let competing_sample_generation = competing_permit.sample_generation_id().to_owned();
    let competing = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &competing_assignment,
                engine_id: crate::journal::request_lifecycle::tests::helpers::TEST_ENGINE_ID,
                docker_root_digest: &competing_docker_root,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
                sample_generation_id: &competing_sample_generation,
            },
            competing_permit,
            competing_admission,
        )
        .await?;
    assert_eq!(competing, ReservationResult::AtCapacity);

    let replay = super::request_and_bind(
        &mut transport,
        "test-admin-token",
        &scope,
        physical.intent_id,
        &runner_name,
        SET_ID,
    )
    .await;
    assert!(matches!(replay, Err(EnsureError::Uncertain)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let rows = scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain));
    drop(scope);
    drop(journal);
    let reopened = Journal::open(&scratch.file()).await?;
    let reopened_scope = reopened.connection_scope(&repository).await?;
    let rows = reopened_scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == physical.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain));
    assert!(
        reopened_scope
            .claim_listener_delivery(physical.reservation_id)
            .await?
            .is_none()
    );
    Ok(())
}
