use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::RepositoryConnection;
use crate::journal::request_lifecycle::tests::helpers::{
    JobFactsReaderFixture, Scratch, binding, created_scope,
};
use crate::journal::{
    CapacityLimits, ConnectionJournal, Journal, PhysicalRunnerJitState, ReservationState,
};
use crate::reconcile::LaunchPhase;
use crate::scale_set::EnsureError;
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

const SET_ID: i64 = 501;
const REQUEST_ID: i64 = 70;

struct ReplyTransport {
    status: u16,
    body: &'static [u8],
    path_suffix: &'static str,
    calls: Arc<AtomicUsize>,
}

impl Transport for ReplyTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        assert!(request.path.ends_with(self.path_suffix));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Exchange {
            status: self.status,
            body: self.body.to_vec(),
        })
    }
}

impl crate::launch::Lane for ReplyTransport {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        Ok(())
    }
}

struct NativeFixture {
    scratch: Scratch,
    journal: Journal,
    scope: ConnectionJournal,
    binding: RepositoryConnection,
    physical: JobFactsReaderFixture,
    policy_generation: u64,
    policy_digest: String,
    runner_name: String,
}

async fn create_native_fixture() -> Result<NativeFixture, String> {
    let scratch = Scratch::new().map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await
        .map_err(|error| error.to_string())?;
    let binding = binding("native-adverse-jit", 101).map_err(|error| error.to_string())?;
    let scope = created_scope(&journal, &binding, SET_ID)
        .await
        .map_err(|error| error.to_string())?;
    let physical = crate::journal::request_lifecycle::tests::helpers::
        provision_acquired_request_with_initialization(&scope, REQUEST_ID, 'e', 'e')
            .await
            .map_err(|error| error.to_string())?;
    let (policy_generation, policy_digest) = scope
        .test_active_authority_identity()
        .await
        .map_err(|error| error.to_string())?;
    Ok(NativeFixture {
        scratch,
        journal,
        scope,
        binding,
        runner_name: format!("v{}", "e".repeat(32)),
        physical,
        policy_generation,
        policy_digest,
    })
}

#[tokio::test]
async fn malformed_conflict_and_forbidden_jit_keep_native_acquire_charge() -> Result<(), String> {
    for (label, status, body, expected) in [
        (
            "malformed",
            200,
            b"not-json".as_slice(),
            EnsureError::Uncertain,
        ),
        ("conflict", 409, b"".as_slice(), EnsureError::Conflict),
        ("forbidden", 403, b"".as_slice(), EnsureError::Forbidden),
    ] {
        assert_jit_failure_holds_native_charge(label, status, body, expected).await?;
    }
    Ok(())
}

async fn assert_jit_failure_holds_native_charge(
    label: &str,
    status: u16,
    body: &'static [u8],
    expected_error: EnsureError,
) -> Result<(), String> {
    let native = create_native_fixture().await?;
    let physical = &native.physical;
    let runner_name = &native.runner_name;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut transport = ReplyTransport {
        status,
        body,
        path_suffix: "/generatejitconfig",
        calls: calls.clone(),
    };
    let result = super::request_and_bind(
        &mut transport,
        "admin-token",
        &native.scope,
        super::JitRequestIdentity {
            intent_id: physical.intent_id,
            reservation_id: physical.reservation_id,
            request_id: REQUEST_ID,
            runner_name,
            scale_set_id: SET_ID,
            policy_generation: native.policy_generation,
            policy_digest: &native.policy_digest,
        },
    )
    .await;
    assert_eq!(result.err(), Some(expected_error), "{label}");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "{label}");
    let reservation_id = physical.reservation_id;
    let intent_id = physical.intent_id;
    let requested_name = runner_name.clone();
    let policy_generation = native.policy_generation;
    let policy_digest = native.policy_digest.clone();
    let charge = (2_000, 8_589_934_592);
    verify_uncertain_charge(&native, reservation_id, charge, label).await?;

    let replay = super::request_and_bind(
        &mut transport,
        "admin-token",
        &native.scope,
        super::JitRequestIdentity {
            intent_id: physical.intent_id,
            reservation_id,
            request_id: REQUEST_ID,
            runner_name,
            scale_set_id: SET_ID,
            policy_generation: native.policy_generation,
            policy_digest: &native.policy_digest,
        },
    )
    .await;
    assert!(matches!(replay, Err(EnsureError::Uncertain)), "{label}");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "{label}");
    let NativeFixture {
        scratch,
        journal,
        scope,
        binding,
        ..
    } = native;
    drop(scope);
    drop(journal);
    verify_reopened_uncertain(
        ReopenedJitAttempt {
            scratch: &scratch,
            binding: &binding,
            reservation_id,
            intent_id,
            requested_name: &requested_name,
            policy_generation,
            policy_digest: &policy_digest,
            label,
        },
        &mut transport,
        &calls,
    )
    .await
}

async fn verify_uncertain_charge(
    native: &NativeFixture,
    reservation_id: i64,
    charge: (u64, u64),
    label: &str,
) -> Result<(), String> {
    let rows = native
        .scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == reservation_id)
        .ok_or_else(|| format!("{label}: reservation is absent"))?;
    assert_eq!(
        row.reservation.state,
        ReservationState::Uncertain,
        "{label}"
    );
    assert_eq!(row.reservation.cpu_millicores, 2_000, "{label}");
    assert_eq!(row.reservation.memory_bytes, 8_589_934_592, "{label}");
    assert_eq!(row.jit_state, PhysicalRunnerJitState::Uncertain, "{label}");
    assert_eq!(row.launch_phase, Some(LaunchPhase::JitUncertain), "{label}");
    assert_eq!(row.runner, None, "{label}");
    assert_eq!(row.reservation.cpu_millicores, charge.0, "{label}");
    assert_eq!(row.reservation.memory_bytes, charge.1, "{label}");
    assert_eq!(row.runner, None, "{label}");
    assert_eq!(
        native
            .journal
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1,
        "{label}"
    );
    assert!(
        native
            .scope
            .claim_listener_delivery(reservation_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "{label}"
    );
    Ok(())
}

struct ReopenedJitAttempt<'a> {
    scratch: &'a Scratch,
    binding: &'a RepositoryConnection,
    reservation_id: i64,
    intent_id: i64,
    requested_name: &'a str,
    policy_generation: u64,
    policy_digest: &'a str,
    label: &'a str,
}

async fn verify_reopened_uncertain(
    attempt: ReopenedJitAttempt<'_>,
    transport: &mut ReplyTransport,
    calls: &AtomicUsize,
) -> Result<(), String> {
    let reopened = Journal::open(&attempt.scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let reopened_scope = reopened
        .connection_scope(attempt.binding)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened_scope
            .request_acquire_state(
                &reopened_scope
                    .assignment_identity(crate::journal::AssignmentKey::AssignedRequest {
                        request_id: REQUEST_ID
                    },)
                    .await
                    .map_err(|error| error.to_string())?
            )
            .await
            .map_err(|error| error.to_string())?,
        Some(crate::journal::RequestAcquireState::Acquired),
        "{}",
        attempt.label
    );
    assert_eq!(
        reopened
            .occupied_reservation_count()
            .await
            .map_err(|error| error.to_string())?,
        1,
        "{}",
        attempt.label
    );
    let rows = reopened_scope
        .physical_runner_reservations()
        .await
        .map_err(|error| error.to_string())?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == attempt.reservation_id)
        .ok_or_else(|| format!("{}: The reopened reservation is absent.", attempt.label))?;
    assert_eq!(row.intent_id, attempt.intent_id, "{}", attempt.label);
    assert_eq!(
        row.reservation.state,
        ReservationState::Uncertain,
        "{}",
        attempt.label
    );
    let replay = super::request_and_bind(
        transport,
        "admin-token",
        &reopened_scope,
        super::JitRequestIdentity {
            intent_id: attempt.intent_id,
            reservation_id: attempt.reservation_id,
            request_id: REQUEST_ID,
            runner_name: attempt.requested_name,
            scale_set_id: SET_ID,
            policy_generation: attempt.policy_generation,
            policy_digest: attempt.policy_digest,
        },
    )
    .await;
    assert!(
        matches!(replay, Err(EnsureError::Uncertain)),
        "{}",
        attempt.label
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "{}", attempt.label);
    assert!(
        reopened_scope
            .claim_listener_delivery(attempt.reservation_id)
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "{}",
        attempt.label
    );
    Ok(())
}
