use crate::error::HostError;
use crate::journal::connection::reservation::post_jit_retirement::test_support;
use crate::journal::connection::storage::read_transaction;
use crate::journal::request_lifecycle::tests::native_started_intake_fixture;
use crate::launch::bootstrap::verify_storage_cleanup_for_scope;
use std::collections::VecDeque;
use velnor_runner_github::{Exchange, Transport, TransportFail, ensure_runner_absent};

#[path = "producer_tests/local_identity.rs"]
mod local_identity;
#[path = "producer_tests/network_failure.rs"]
mod network_failure;
#[path = "producer_tests/network_id.rs"]
mod network_id;
#[path = "producer_tests/retry.rs"]
mod retry;

#[test]
fn local_retirement_proof_is_recorded_with_observer_cleanup() -> Result<(), HostError> {
    retry::run_on_test_stack(|| async {
        local_retirement_proof_is_recorded_with_observer_cleanup_inner().await
    })
}

async fn local_retirement_proof_is_recorded_with_observer_cleanup_inner() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    test_support::recover_fixture(&fixture).await?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        !fixture
            .scope
            .resume_retirement_local_absence(&claim)
            .await?
    );
    let proof =
        verify_storage_cleanup_for_scope(&fixture.scope, &fixture.journal, fixture.reservation_id)
            .await?;

    fixture
        .scope
        .record_retirement_local_absence(&claim, proof)
        .await?;
    assert!(
        fixture
            .scope
            .resume_retirement_local_absence(&claim)
            .await?
    );
    assert_retirement_storage_state(&fixture.journal, fixture.reservation_id, 1).await?;
    let reopened = crate::journal::Journal::open(&fixture.database_path).await?;
    assert_retirement_storage_state(&reopened, fixture.reservation_id, 1).await?;
    Ok(())
}

#[tokio::test]
async fn local_retirement_phase_failure_rolls_back_observed_cleanup() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    test_support::recover_fixture(&fixture).await?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let proof =
        verify_storage_cleanup_for_scope(&fixture.scope, &fixture.journal, fixture.reservation_id)
            .await?;
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "CREATE TRIGGER reject_retirement_local_phase BEFORE INSERT ON physical_attempt_post_jit_retirement_progress WHEN NEW.phase_index = 1 BEGIN SELECT RAISE(ABORT, 'test rollback'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;

    assert!(
        fixture
            .scope
            .record_retirement_local_absence(&claim, proof)
            .await
            .is_err()
    );
    connection
        .execute("DROP TRIGGER reject_retirement_local_phase", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_retirement_storage_state(&fixture.journal, fixture.reservation_id, 0).await?;
    Ok(())
}

#[tokio::test]
async fn exact_remote_absence_receipt_advances_phase_two() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    test_support::recover_fixture(&fixture).await?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let proof =
        verify_storage_cleanup_for_scope(&fixture.scope, &fixture.journal, fixture.reservation_id)
            .await?;
    fixture
        .scope
        .record_retirement_local_absence(&claim, proof)
        .await?;

    let mut transport = ScriptedTransport::new([(200, br#"{"count":0,"value":[]}"#.to_vec())]);
    let receipt = ensure_runner_absent(&mut transport, &fixture.runner, "test-admin-token")
        .map_err(|_| HostError::Identity)?;
    fixture
        .scope
        .record_mocked_retirement_remote_absence(&claim, &receipt)
        .await?;
    fixture
        .scope
        .record_retirement_storage_phase(&claim)
        .await?;

    read_transaction(&fixture.journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT reservation.remote_cleanup_proven,receipt.remove_status,receipt.lookup_status,receipt.lookup_count,receipt.lookup_value_count,phase.phase,(SELECT phase3.phase FROM physical_attempt_post_jit_retirement_progress phase3 WHERE phase3.reservation_id=reservation.id AND phase3.claim_generation=receipt.claim_generation AND phase3.phase_index=3) FROM capacity_reservations reservation JOIN physical_attempt_post_jit_retirement_remote_absence receipt ON receipt.reservation_id=reservation.id JOIN physical_attempt_post_jit_retirement_progress phase ON phase.reservation_id=receipt.reservation_id AND phase.claim_generation=receipt.claim_generation AND phase.phase_index=2 WHERE reservation.id=?1",
                [fixture.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let remote_cleanup: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let remove_status: Option<i64> = row.get(1).map_err(|_| HostError::Journal)?;
        let lookup_status: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let lookup_count: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        let value_count: i64 = row.get(4).map_err(|_| HostError::Journal)?;
        let phase: String = row.get(5).map_err(|_| HostError::Journal)?;
        let storage_phase: String = row.get(6).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || remote_cleanup != 1
            || remove_status.is_some()
            || lookup_status != 200
            || lookup_count != 0
            || value_count != 0
            || phase != "remote_absent"
            || storage_phase != "storage_absent"
        {
            return Err(HostError::Identity);
        }
        Ok(())
    })
    .await
}

#[tokio::test]
async fn rejected_delete_receipt_does_not_advance_retirement() -> Result<(), HostError> {
    let fixture = native_started_intake_fixture().await?;
    test_support::recover_fixture(&fixture).await?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let proof =
        verify_storage_cleanup_for_scope(&fixture.scope, &fixture.journal, fixture.reservation_id)
            .await?;
    fixture
        .scope
        .record_retirement_local_absence(&claim, proof)
        .await?;

    let runner = serde_json::json!({
        "count": 1,
        "value": [{
            "id": fixture.runner.id,
            "name": fixture.runner.name,
            "runnerScaleSetId": fixture.runner.runner_scale_set_id
        }]
    });
    let empty = br#"{"count":0,"value":[]}"#.to_vec();
    let mut transport = ScriptedTransport::new([
        (
            200,
            serde_json::to_vec(&runner).map_err(|_| HostError::Journal)?,
        ),
        (404, Vec::new()),
        (200, empty),
    ]);
    let receipt = ensure_runner_absent(&mut transport, &fixture.runner, "test-admin-token")
        .map_err(|_| HostError::Identity)?;
    assert_eq!(
        receipt.removal_status(),
        velnor_runner_github::RunnerRemovalStatus::AbsentAfterRejectedRemoval
    );
    assert!(
        fixture
            .scope
            .record_mocked_retirement_remote_absence(&claim, &receipt)
            .await
            .is_err()
    );
    read_transaction(&fixture.journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT remote_cleanup_proven,(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_remote_absence WHERE reservation_id=?1),(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress WHERE reservation_id=?1 AND phase_index=2) FROM capacity_reservations WHERE id=?1",
                [fixture.reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let remote_cleanup: i64 = row.get(0).map_err(|_| HostError::Journal)?;
        let receipts: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let remote_phases: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        if remote_cleanup != 0 || receipts != 0 || remote_phases != 0 {
            return Err(HostError::Identity);
        }
        Ok(())
    })
    .await
}

#[tokio::test]
async fn network_removal_and_phase_four_commit_together() -> Result<(), HostError> {
    Box::pin(network_removal_and_phase_four_commit_together_inner()).await
}

async fn network_removal_and_phase_four_commit_together_inner() -> Result<(), HostError> {
    let fixture = Box::pin(crate::launch::bootstrap::cleanup_runtime_fixture()).await?;
    let selected = Box::pin(selected_engine(&fixture.scope, fixture.reservation_id)).await?;
    let claim = Box::pin(prepare_local_remote(&fixture)).await?;
    Box::pin(fixture.scope.record_retirement_storage_phase(&claim)).await?;
    Box::pin(remove_network_for_retirement(&fixture, &claim, &selected)).await?;
    Box::pin(assert_network_phase(&fixture, "removed", 1)).await
}

#[tokio::test]
async fn network_phase_failure_keeps_reservation_charged_and_uncertain() -> Result<(), HostError> {
    Box::pin(network_phase_failure_keeps_reservation_charged_and_uncertain_inner()).await
}

async fn network_phase_failure_keeps_reservation_charged_and_uncertain_inner()
-> Result<(), HostError> {
    let fixture = Box::pin(crate::launch::bootstrap::cleanup_runtime_fixture()).await?;
    let selected = Box::pin(selected_engine(&fixture.scope, fixture.reservation_id)).await?;
    let claim = Box::pin(prepare_local_remote(&fixture)).await?;
    Box::pin(fixture.scope.record_retirement_storage_phase(&claim)).await?;
    let connection = fixture.journal.connection().await?;
    connection
        .execute(
            "CREATE TRIGGER reject_retirement_network_phase BEFORE INSERT ON physical_attempt_post_jit_retirement_progress WHEN NEW.phase_index = 4 BEGIN SELECT RAISE(ABORT, 'test rollback'); END",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(
        Box::pin(remove_network_for_retirement(&fixture, &claim, &selected))
            .await
            .is_err()
    );
    connection
        .execute("DROP TRIGGER reject_retirement_network_phase", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Box::pin(assert_network_phase(&fixture, "uncertain", 0)).await
}

async fn prepare_local_remote(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
) -> Result<super::claim::PostJitRetirementClaim, HostError> {
    test_support::recover_fixture(&fixture.base).await?;
    let claim = fixture
        .scope
        .claim_post_jit_retirement(fixture.reservation_id, 60)
        .await?
        .ok_or(HostError::Journal)?;
    let proof = Box::pin(verify_storage_cleanup_for_scope(
        &fixture.scope,
        &fixture.journal,
        fixture.reservation_id,
    ))
    .await?;
    Box::pin(fixture.scope.record_retirement_local_absence(&claim, proof)).await?;
    let mut transport = ScriptedTransport::new([(200, br#"{"count":0,"value":[]}"#.to_vec())]);
    let receipt = ensure_runner_absent(&mut transport, &fixture.base.runner, "test-admin-token")
        .map_err(|_| HostError::Identity)?;
    Box::pin(
        fixture
            .scope
            .record_mocked_retirement_remote_absence(&claim, &receipt),
    )
    .await?;
    Ok(claim)
}

async fn remove_network_for_retirement(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    claim: &super::claim::PostJitRetirementClaim,
    selected: &crate::launch::SelectedEngineContext,
) -> Result<(), HostError> {
    crate::launch::bootstrap::remove_owned_network_for_retirement(
        &fixture.server.docker(),
        &fixture.scope,
        selected,
        fixture.reservation_id,
        claim,
    )
    .await
}

async fn selected_engine(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
) -> Result<crate::launch::SelectedEngineContext, HostError> {
    let identity = Box::pin(scope.storage_cleanup_identity(reservation_id))
        .await?
        .ok_or(HostError::Journal)?;
    let engine = Box::pin(scope.engine_context())
        .await?
        .ok_or(HostError::Journal)?;
    let images = crate::launch::verified_images::test_worker_images_for(
        identity.engine_id(),
        identity.docker_root_digest(),
        "x86_64",
        "linux/amd64",
    );
    crate::launch::SelectedEngineContext::bind(&engine, &images)
}

async fn assert_network_phase(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    expected_remove_state: &str,
    expected_phase_count: i64,
) -> Result<(), HostError> {
    network_id::assert_network_phase(fixture, expected_remove_state, expected_phase_count).await
}

struct ScriptedTransport {
    replies: VecDeque<(u16, Vec<u8>)>,
}

impl ScriptedTransport {
    fn new(replies: impl IntoIterator<Item = (u16, Vec<u8>)>) -> Self {
        Self {
            replies: replies.into_iter().collect(),
        }
    }
}

impl Transport for ScriptedTransport {
    fn exchange(
        &mut self,
        _request: &velnor_runner_github::SessionRequest,
    ) -> Result<Exchange, TransportFail> {
        let (status, body) = self.replies.pop_front().ok_or(TransportFail::Reset)?;
        Ok(Exchange { status, body })
    }
}

async fn assert_retirement_storage_state(
    journal: &crate::journal::Journal,
    reservation_id: i64,
    expected_storage_cleanup: i64,
) -> Result<(), HostError> {
    read_transaction(journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT r.state,r.local_cleanup_proven,s.storage_cleanup_proven,(SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups cleanup WHERE cleanup.reservation_id=r.id),(SELECT COUNT(*) FROM physical_attempt_post_jit_retirement_progress phase WHERE phase.reservation_id=r.id AND phase.phase_index=1) FROM capacity_reservations r JOIN capacity_reservation_provisioning_state s ON s.reservation_id=r.id WHERE r.id=?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let state: String = row.get(0).map_err(|_| HostError::Journal)?;
        let local_cleanup: i64 = row.get(1).map_err(|_| HostError::Journal)?;
        let provisioning_storage_cleanup: i64 = row.get(2).map_err(|_| HostError::Journal)?;
        let cleanup_rows: i64 = row.get(3).map_err(|_| HostError::Journal)?;
        let phase_rows: i64 = row.get(4).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || state != "recovering"
            || local_cleanup != expected_storage_cleanup
            || provisioning_storage_cleanup != expected_storage_cleanup
            || cleanup_rows != expected_storage_cleanup
            || phase_rows != expected_storage_cleanup
        {
            return Err(HostError::Identity);
        }
        Ok(())
    })
    .await
}
