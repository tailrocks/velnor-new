use std::future::Future;
use std::time::Duration;

use crate::error::HostError;
use crate::journal::connection::reservation::post_jit_retirement::claim::PostJitRetirementClaim;
use crate::journal::connection::storage::read_transaction;
use crate::launch::{SelectedEngineContext, bootstrap::cleanup_runtime_fixture};

use super::{
    ScriptedTransport, ensure_runner_absent, remove_network_for_retirement, selected_engine,
    test_support, verify_storage_cleanup_for_scope,
};

#[test]
fn progressed_claim_reuses_local_and_network_proofs_after_expiry() -> Result<(), HostError> {
    run_on_test_stack(progressed_claim_reuses_local_and_network_proofs_after_expiry_inner)
}

pub(super) fn run_on_test_stack<F>(
    future: impl FnOnce() -> F + Send + 'static,
) -> Result<(), HostError>
where
    F: Future<Output = Result<(), HostError>> + Send + 'static,
{
    std::thread::Builder::new()
        .name("retirement-test-runtime".to_owned())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| HostError::Journal)?
                .block_on(future())
        })
        .map_err(|_| HostError::Journal)?
        .join()
        .map_err(|_| HostError::Journal)?
}

async fn progressed_claim_reuses_local_and_network_proofs_after_expiry_inner()
-> Result<(), HostError> {
    let fixture = cleanup_runtime_fixture().await?;
    let selected = selected_engine(&fixture.scope, fixture.reservation_id).await?;
    test_support::recover_fixture(&fixture.base).await?;
    let first = record_first_generation(&fixture, &selected).await?;
    tokio::time::sleep(Duration::from_secs(11)).await;
    let candidate = fixture
        .scope
        .recoverable_post_jit_retirements()
        .await?
        .into_iter()
        .next()
        .ok_or(HostError::Journal)?;
    let retry = fixture
        .scope
        .claim_recoverable_post_jit_retirement(&candidate, 60)
        .await?
        .ok_or(HostError::Journal)?;
    if retry.claim_generation() != first.claim_generation() + 1 {
        return Err(HostError::Identity);
    }
    resume_retry_generation(&fixture, &retry).await?;
    assert_retry_state(&fixture).await
}

async fn record_first_generation(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    selected: &SelectedEngineContext,
) -> Result<PostJitRetirementClaim, HostError> {
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 10)
        .await?
        .ok_or(HostError::Journal)?;
    let proof =
        verify_storage_cleanup_for_scope(&fixture.scope, &fixture.journal, fixture.reservation_id)
            .await?;
    fixture
        .scope
        .record_retirement_local_absence(&claim, proof)
        .await?;
    let receipt = absent_receipt(&fixture.base.runner)?;
    fixture
        .scope
        .record_mocked_retirement_remote_absence(&claim, &receipt)
        .await?;
    fixture
        .scope
        .record_retirement_storage_phase(&claim)
        .await?;
    remove_network_for_retirement(fixture, &claim, selected).await?;
    Ok(claim)
}

fn absent_receipt(
    runner: &velnor_runner_github::RunnerReference,
) -> Result<velnor_runner_github::RunnerAbsenceReceipt, HostError> {
    let mut transport = ScriptedTransport::new([(200, br#"{"count":0,"value":[]}"#.to_vec())]);
    ensure_runner_absent(&mut transport, runner, "test-admin-token")
        .map_err(|_| HostError::Identity)
}

async fn resume_retry_generation(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    claim: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    if !fixture.scope.resume_retirement_local_absence(claim).await? {
        return Err(HostError::Identity);
    }
    let receipt = absent_receipt(&fixture.base.runner)?;
    fixture
        .scope
        .record_mocked_retirement_remote_absence(claim, &receipt)
        .await?;
    fixture.scope.record_retirement_storage_phase(claim).await?;
    if !fixture
        .scope
        .resume_retirement_network_absence(claim)
        .await?
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

async fn assert_retry_state(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> Result<(), HostError> {
    read_transaction(&fixture.journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT reservation.state,reservation.local_cleanup_proven,reservation.remote_cleanup_proven,state.jit_state,jit.state,(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=reservation.id AND phase.claim_generation=1 AND phase.phase_index=4),(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=reservation.id AND phase.claim_generation=2 AND phase.phase_index=4),(SELECT COUNT(*) FROM physical_attempt_post_jit_retirements terminal WHERE terminal.reservation_id=reservation.id) FROM capacity_reservations reservation JOIN capacity_reservation_provisioning_state state ON state.reservation_id=reservation.id JOIN connection_runner_jits jit ON jit.intent_id=reservation.intent_id AND jit.connection_id=reservation.connection_id WHERE reservation.id=?1",
                [fixture.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let state: String = row.get(0).map_err(|_| HostError::Journal)?;
        let local: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let remote: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let jit: String = row.get(3).map_err(|_| HostError::Journal)?;
        let runner: String = row.get(4).map_err(|_| HostError::Journal)?;
        let old_phase: i64 = row.get(5).map_err(|_| HostError::Journal)?;
        let new_phase: i64 = row.get(6).map_err(|_| HostError::Journal)?;
        let terminal: i64 = row.get(7).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || !matches!(state.as_str(), "reserved" | "uncertain" | "recovering")
            || local != 1
            || remote != 1
            || jit != "cleaned"
            || runner != "cleaned"
            || old_phase != 1
            || new_phase != 1
            || terminal != 0
        {
            return Err(HostError::Identity);
        }
        Ok(())
    })
    .await
}
