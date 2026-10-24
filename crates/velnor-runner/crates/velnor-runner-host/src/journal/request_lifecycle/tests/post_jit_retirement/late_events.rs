use crate::journal::{
    MetadataSettlementResult,
    connection::storage::{in_transaction, read_transaction},
};
use std::future::Future;

use super::{
    assert_terminal_state, prepared_fixture, record_storage_and_network_proofs,
    seed_mocked_external_evidence,
};
use crate::error::HostError;

/// Create a test fixture with a recorded retirement claim.
pub(crate) async fn claimed_native_fixture()
-> Result<super::super::NativeStartedIntakeFixture, HostError> {
    let fixture = prepared_fixture().await?;
    seed_mocked_external_evidence(&fixture).await?;
    Ok(fixture)
}

pub(crate) async fn assert_historical_intake_state(
    fixture: &super::super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    assert_historical_scope_state(
        &fixture.scope,
        fixture.runner.runner_scale_set_id,
        fixture.request_id,
        fixture.intent_id,
    )
    .await
}

pub(crate) async fn assert_historical_scope_state(
    scope: &crate::journal::ConnectionJournal,
    scale_set_id: i64,
    request_id: i64,
    intent_id: i64,
) -> Result<(), HostError> {
    in_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT auth.started_event_digest,auth.completed_event_digest,j.state,j.started_request_id,j.completed_request_id,i.runner_request_id,(SELECT COUNT(*) FROM connection_completion_cleanup c WHERE c.intent_id=?4) FROM connection_request_authorizations auth JOIN connection_runner_jits j ON j.intent_id=?4 JOIN connection_intents i ON i.id=?4 WHERE auth.connection_id=?1 AND auth.scale_set_id=?2 AND auth.request_id=?3 AND j.connection_id=?1 AND j.scale_set_id=?2 AND i.connection_id=?1 AND i.scale_set_id=?2",
                (scope.connection_id().as_str(), scale_set_id, request_id, intent_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let started: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
        let completed: Option<String> = row.get(1).map_err(|_| HostError::Journal)?;
        let jit_state: String = row.get(2).map_err(|_| HostError::Journal)?;
        let jit_started: Option<i64> = row.get(3).map_err(|_| HostError::Journal)?;
        let jit_completed: Option<i64> = row.get(4).map_err(|_| HostError::Journal)?;
        let runner_request: Option<i64> = row.get(5).map_err(|_| HostError::Journal)?;
        let cleanup_count: i64 = row.get(6).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || started.is_none()
            || completed.is_none()
            || !matches!(jit_state.as_str(), "ready" | "started" | "cleaned")
            || jit_started.is_some()
            || jit_completed.is_some()
            || runner_request.is_some()
            || cleanup_count != 0
        {
            return Err(HostError::Journal);
        }
        Ok(())
    })
    .await
}

pub(crate) async fn assert_actual_request_owns_event(
    fixture: &super::super::NativeStartedIntakeFixture,
    actual_request_id: i64,
) -> Result<(), HostError> {
    read_transaction(fixture.scope.journal_ref(), async |connection| {
        let mut rows = connection
            .query(
                "SELECT request_id,started_event_digest FROM connection_request_authorizations WHERE connection_id=?1 AND scale_set_id=?2 AND request_id IN (?3,?4) ORDER BY request_id",
                (fixture.scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, fixture.request_id, actual_request_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let witness = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let witness_id: i64 = witness.get(0).map_err(|_| HostError::Journal)?;
        let witness_started: Option<String> = witness.get(1).map_err(|_| HostError::Journal)?;
        let actual = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let actual_id: i64 = actual.get(0).map_err(|_| HostError::Journal)?;
        let actual_started: Option<String> = actual.get(1).map_err(|_| HostError::Journal)?;
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some()
            || witness_id != fixture.request_id
            || witness_started.is_some()
            || actual_id != actual_request_id
            || actual_started.is_none()
        {
            return Err(HostError::Journal);
        }
        Ok(())
    })
    .await
}

async fn retired_native_fixture() -> Result<super::super::NativeStartedIntakeFixture, HostError> {
    let fixture = claimed_native_fixture().await?;
    record_storage_and_network_proofs(&fixture).await?;
    let claim =
        super::helpers::create_cleaned_settlement_probe(&fixture.scope, &fixture.provisioned, 'c')
            .await?;
    let sample = super::helpers::settlement_sample(&fixture.scope, &claim).await?;
    if fixture
        .scope
        .record_storage_metadata_settlement(&claim, sample)
        .await?
        != MetadataSettlementResult::Settled
    {
        return Err(HostError::Journal);
    }
    assert_terminal_state(&fixture).await?;
    Ok(fixture)
}

#[path = "late_events/active.rs"]
mod active;
#[path = "late_events/crossed.rs"]
mod crossed;
#[path = "late_events/terminal.rs"]
mod terminal;
#[path = "late_events/validation.rs"]
mod validation;

async fn assert_no_historical_cleanup(
    connection: &turso::Connection,
    intent_id: i64,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM connection_completion_cleanup WHERE intent_id=?1",
            [intent_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    if row.get::<i64>(0).map_err(|_| HostError::Journal)? == 0 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn on_large_test_stack(
    future: impl Future<Output = Result<(), HostError>> + Send + 'static,
) -> Result<(), HostError> {
    std::thread::Builder::new()
        .stack_size(8_388_608)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| HostError::Journal)?;
            runtime.block_on(future)
        })
        .map_err(|_| HostError::Journal)?
        .join()
        .map_err(|_| HostError::Journal)?
}
