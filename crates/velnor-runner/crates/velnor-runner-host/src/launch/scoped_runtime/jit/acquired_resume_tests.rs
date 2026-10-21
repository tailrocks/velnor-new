use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    error::HostError,
    journal::{CapacityLimits, PhysicalRunnerJitState, RequestAcquireState, ReservationState},
    launch::Lane,
    scale_set::EnsureError,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

struct SuccessfulJitTransport(Arc<AtomicUsize>);

impl Transport for SuccessfulJitTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        assert!(request.path.ends_with("/generatejitconfig"));
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Exchange {
            status: 200,
            body: serde_json::json!({
                "encodedJITConfig": "opaque-jit",
                "runner": {
                    "id": 9_001,
                    "name": format!("v{}", "e".repeat(32)),
                    "runnerScaleSetId": 501
                }
            })
            .to_string()
            .into_bytes(),
        })
    }
}

impl Lane for SuccessfulJitTransport {
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
async fn acquired_physical_job_binds_one_exact_jit_and_rejects_replay() -> Result<(), HostError> {
    const REQUEST_ID: i64 = 70;

    use crate::journal::request_lifecycle::tests::helpers::{
        Scratch, binding, created_scope, provision_acquired_request_with_initialization,
    };

    let scratch = Scratch::new()?;
    let journal = crate::Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let repository = binding("acquired-jit-resume", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let fixture =
        provision_acquired_request_with_initialization(&scope, REQUEST_ID, 'e', 'e').await?;
    let (policy_generation, policy_digest) = scope.test_active_authority_identity().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut transport = SuccessfulJitTransport(Arc::clone(&calls));
    let runner_name = format!("v{}", "e".repeat(32));

    let (runner, _encoded_jit) = super::request_and_bind(
        &mut transport,
        "test-admin-token",
        &scope,
        super::JitRequestIdentity {
            intent_id: fixture.intent_id,
            reservation_id: fixture.reservation_id,
            request_id: REQUEST_ID,
            runner_name: &runner_name,
            scale_set_id: 501,
            policy_generation,
            policy_digest: &policy_digest,
        },
    )
    .await
    .map_err(|_| HostError::Journal)?;
    assert_eq!(runner.id, 9_001);
    assert_eq!(runner.name, runner_name);
    assert_eq!(runner.runner_scale_set_id, 501);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    assert_eq!(
        scope.request_acquire_state(&fixture.assignment).await?,
        Some(RequestAcquireState::Acquired)
    );
    let rows = scope.physical_runner_reservations().await?;
    let row = rows
        .iter()
        .find(|row| row.reservation_id == fixture.reservation_id)
        .ok_or(HostError::Journal)?;
    assert_eq!(row.reservation.state, ReservationState::Reserved);
    assert_eq!(row.jit_state, PhysicalRunnerJitState::Bound);
    assert_eq!(row.witness_request_id, Some(REQUEST_ID));
    assert_eq!(row.runner.as_ref(), Some(&runner));
    assert_eq!(rows.len(), 1);

    let replay = super::request_and_bind(
        &mut transport,
        "test-admin-token",
        &scope,
        super::JitRequestIdentity {
            intent_id: fixture.intent_id,
            reservation_id: fixture.reservation_id,
            request_id: REQUEST_ID,
            runner_name: &runner_name,
            scale_set_id: 501,
            policy_generation,
            policy_digest: &policy_digest,
        },
    )
    .await;
    assert!(matches!(replay, Err(EnsureError::Uncertain)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let replay_rows = scope.physical_runner_reservations().await?;
    assert_eq!(replay_rows.len(), 1);
    assert_eq!(replay_rows[0].reservation.state, ReservationState::Reserved);
    Ok(())
}
