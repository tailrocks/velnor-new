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

const SET_ID: i64 = 501;

#[test]
fn pending_jit_bootstrap_error_stays_bounded() {
    assert!(std::mem::size_of::<super::PendingJitBootstrap>() <= 128);
}

#[test]
fn pending_channel_bootstrap_error_stays_bounded() {
    assert!(std::mem::size_of::<super::PendingChannelBootstrap>() <= 128);
}

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
}

struct StatusTransport {
    status: u16,
    calls: Arc<AtomicUsize>,
}

impl Transport for StatusTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        assert!(request.path.ends_with("/generatejitconfig"));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Exchange {
            status: self.status,
            body: Vec::new(),
        })
    }
}

impl Lane for StatusTransport {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

#[tokio::test]
async fn jit_conflict_keeps_resources_charged_and_never_replays() -> Result<(), HostError> {
    use crate::journal::Journal;
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, provision_acquired_request_with_initialization,
    };

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
    let physical = provision_acquired_request_with_initialization(&scope, 70, 'b', 'b').await?;
    let runner_name = format!("v{}", "b".repeat(32));
    let (policy_generation, policy_digest) = scope.test_active_authority_identity().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut transport = ConflictTransport(calls.clone());
    let attempt = JitConflictAttempt {
        scope: &scope,
        intent_id: physical.intent_id,
        reservation_id: physical.reservation_id,
        runner_name,
        policy_generation,
        policy_digest,
    };

    assert_initial_conflict(&attempt, &mut transport, &calls).await?;
    assert_competing_request_is_rejected_at_capacity(&scope).await?;
    assert_replay_is_uncertain(&attempt, &mut transport, &calls).await?;
    drop(attempt);
    drop(scope);
    drop(journal);
    assert_reopened_conflict_stays_uncertain(&scratch, &repository, physical.reservation_id)
        .await?;
    Ok(())
}

#[tokio::test]
async fn non_409_jit_conflict_errors_do_not_record_conflict_receipts() -> Result<(), HostError> {
    use crate::journal::Journal;
    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, provision_acquired_request_with_initialization,
    };

    for (repository_name, request_id, status) in [
        ("jit-non-conflict", 71, 403),
        ("jit-malformed-success", 72, 200),
    ] {
        let scratch = Scratch::new()?;
        let journal = Journal::open(&scratch.file()).await?;
        journal
            .configure_capacity(CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            })
            .await?;
        let repository = binding(repository_name, 101)?;
        let scope = created_scope(&journal, &repository, SET_ID).await?;
        let physical =
            provision_acquired_request_with_initialization(&scope, request_id, 'c', 'c').await?;
        let runner_name = format!("v{}", "c".repeat(32));
        let (policy_generation, policy_digest) = scope.test_active_authority_identity().await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut transport = StatusTransport {
            status,
            calls: calls.clone(),
        };
        let result = super::request_and_bind(
            &mut transport,
            "test-admin-token",
            &scope,
            super::JitRequestIdentity {
                intent_id: physical.intent_id,
                reservation_id: physical.reservation_id,
                request_id,
                runner_name: &runner_name,
                scale_set_id: SET_ID,
                policy_generation,
                policy_digest: &policy_digest,
            },
        )
        .await;
        if status == 403 {
            assert!(matches!(result, Err(EnsureError::Forbidden)));
        } else {
            assert!(result.is_err());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            scope
                .test_jit_conflict_receipt_count(physical.intent_id)
                .await?,
            0
        );
        let rows = scope.physical_runner_reservations().await?;
        let row = rows
            .iter()
            .find(|row| row.reservation_id == physical.reservation_id)
            .ok_or(HostError::Journal)?;
        assert_eq!(row.reservation.state, ReservationState::Uncertain);
    }
    Ok(())
}

struct JitConflictAttempt<'a> {
    scope: &'a crate::journal::ConnectionJournal,
    intent_id: i64,
    reservation_id: i64,
    runner_name: String,
    policy_generation: u64,
    policy_digest: String,
}

async fn assert_initial_conflict(
    attempt: &JitConflictAttempt<'_>,
    transport: &mut ConflictTransport,
    calls: &AtomicUsize,
) -> Result<(), HostError> {
    let result = super::request_and_bind(
        transport,
        "test-admin-token",
        attempt.scope,
        super::JitRequestIdentity {
            intent_id: attempt.intent_id,
            reservation_id: attempt.reservation_id,
            request_id: 70,
            runner_name: &attempt.runner_name,
            scale_set_id: SET_ID,
            policy_generation: attempt.policy_generation,
            policy_digest: &attempt.policy_digest,
        },
    )
    .await;
    assert!(matches!(result, Err(EnsureError::Conflict)));

    let rows = attempt.scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == attempt.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(row.runner, None);
    assert_eq!(row.witness_request_id, Some(70));
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain));
    assert_eq!(
        attempt
            .scope
            .test_jit_conflict_receipt_count(attempt.intent_id)
            .await?,
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        attempt
            .scope
            .claim_listener_delivery(attempt.reservation_id)
            .await?
            .is_none()
    );
    Ok(())
}

async fn assert_competing_request_is_rejected_at_capacity(
    scope: &crate::journal::ConnectionJournal,
) -> Result<(), HostError> {
    use crate::journal::{
        AssignmentKey, ReservationRequest, ReservationResult,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

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
    Ok(())
}

async fn assert_replay_is_uncertain(
    attempt: &JitConflictAttempt<'_>,
    transport: &mut ConflictTransport,
    calls: &AtomicUsize,
) -> Result<(), HostError> {
    let replay = super::request_and_bind(
        transport,
        "test-admin-token",
        attempt.scope,
        super::JitRequestIdentity {
            intent_id: attempt.intent_id,
            reservation_id: attempt.reservation_id,
            request_id: 70,
            runner_name: &attempt.runner_name,
            scale_set_id: SET_ID,
            policy_generation: attempt.policy_generation,
            policy_digest: &attempt.policy_digest,
        },
    )
    .await;
    assert!(matches!(replay, Err(EnsureError::Uncertain)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let rows = attempt.scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == attempt.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain));
    Ok(())
}

async fn assert_reopened_conflict_stays_uncertain(
    scratch: &crate::journal::request_lifecycle::tests::helpers::Scratch,
    repository: &crate::config::RepositoryConnection,
    reservation_id: i64,
) -> Result<(), HostError> {
    use crate::journal::Journal;

    let reopened = Journal::open(&scratch.file()).await?;
    let reopened_scope = reopened.connection_scope(repository).await?;
    let rows = reopened_scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Uncertain);
    assert_eq!(format!("{:?}", row.jit_state), "Uncertain");
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain));
    assert!(
        reopened_scope
            .claim_listener_delivery(reservation_id)
            .await?
            .is_none()
    );
    Ok(())
}
