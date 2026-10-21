use crate::error::HostError;
use crate::journal::connection::storage::in_transaction;
use std::{future::Future, pin::Pin};

use super::{on_large_test_stack, retired_native_fixture};

#[test]
fn retired_completion_reopen_rejects_wrong_runner_relation() -> Result<(), HostError> {
    on_large_test_stack(reopen_rejects_wrong_runner_relation())
}

async fn reopen_rejects_wrong_runner_relation() -> Result<(), HostError> {
    let fixture = completed_retired_fixture().await?;
    in_transaction(fixture.scope.journal_ref(), async |connection| {
        connection
            .execute(
                "UPDATE connection_request_authorizations SET completed_runner_id=?1 WHERE connection_id=?2 AND scale_set_id=?3 AND request_id=?4",
                (fixture.runner.id + 1, fixture.scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, fixture.request_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await?;
    reopen_rejects(fixture).await
}

#[test]
fn retired_completion_reopen_rejects_wrong_intent_relation() -> Result<(), HostError> {
    on_large_test_stack(reopen_rejects_wrong_intent_relation())
}

async fn reopen_rejects_wrong_intent_relation() -> Result<(), HostError> {
    let fixture = completed_retired_fixture().await?;
    let successor = super::super::super::helpers::provision_acquired_request_with_initialization(
        &fixture.scope,
        8_000_002,
        'e',
        'e',
    )
    .await?;
    in_transaction(fixture.scope.journal_ref(), async |connection| {
        connection
            .execute(
                "UPDATE connection_request_authorizations SET completed_intent_id=?1 WHERE connection_id=?2 AND scale_set_id=?3 AND request_id=?4",
                (successor.intent_id, fixture.scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, fixture.request_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await?;
    reopen_rejects(fixture).await
}

#[test]
fn retired_completion_reopen_rejects_invalid_source() -> Result<(), HostError> {
    on_large_test_stack(reopen_rejects_invalid_source())
}

async fn reopen_rejects_invalid_source() -> Result<(), HostError> {
    let fixture = completed_retired_fixture().await?;
    in_transaction(fixture.scope.journal_ref(), async |connection| {
        connection
            .execute(
                "UPDATE connection_request_authorizations SET source_digest=?1 WHERE connection_id=?2 AND scale_set_id=?3 AND request_id=?4",
                ("0".repeat(64), fixture.scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, fixture.request_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await?;
    reopen_rejects(fixture).await
}

#[test]
fn retired_completion_reopen_rejects_wrong_binding() -> Result<(), HostError> {
    on_large_test_stack(reopen_rejects_wrong_binding())
}

async fn reopen_rejects_wrong_binding() -> Result<(), HostError> {
    let fixture = completed_retired_fixture().await?;
    in_transaction(fixture.scope.journal_ref(), async |connection| {
        connection
            .execute(
                "UPDATE connection_request_authorizations SET binding_digest=?1 WHERE connection_id=?2 AND scale_set_id=?3 AND request_id=?4",
                ("0".repeat(64), fixture.scope.connection_id().as_str(), fixture.runner.runner_scale_set_id, fixture.request_id),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    })
    .await?;
    reopen_rejects(fixture).await
}

fn completed_retired_fixture() -> Pin<
    Box<
        dyn Future<Output = Result<super::super::super::NativeStartedIntakeFixture, HostError>>
            + Send
            + 'static,
    >,
> {
    Box::pin(async {
        let fixture = retired_native_fixture().await?;
        fixture
            .scope
            .record_scoped_job_started(
                &fixture.assignment,
                9_360,
                &fixture.source,
                &fixture.runner,
                Some("retired-validation-job"),
            )
            .await?;
        fixture
            .scope
            .record_scoped_job_completed(
                &fixture.assignment,
                9_361,
                &fixture.source,
                Some("Succeeded"),
                Some(&fixture.runner),
                Some("retired-validation-job"),
            )
            .await?;
        Ok(fixture)
    })
}

async fn reopen_rejects(
    fixture: super::super::super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let path = fixture.database_path.clone();
    let super::super::super::NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);
    if crate::journal::Journal::open(&path).await.is_err() {
        let _keep_scratch_until_reopen_finishes = scratch;
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
