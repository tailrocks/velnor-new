use crate::error::HostError;
use crate::journal::connection::storage::{in_transaction, read_transaction};
use crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture;

#[test]
fn late_event_after_retirement_does_not_replace_a_successor_attempt() -> Result<(), HostError> {
    super::on_large_test_stack(
        late_event_after_retirement_does_not_replace_a_successor_attempt_inner(),
    )
}

async fn late_event_after_retirement_does_not_replace_a_successor_attempt_inner()
-> Result<(), HostError> {
    let fixture = super::retired_native_fixture().await?;
    if fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .is_some()
    {
        return Err(HostError::Journal);
    }
    let successor = super::super::helpers::provision_acquired_request_with_initialization(
        &fixture.scope,
        8_000_001,
        'd',
        'd',
    )
    .await?;
    let successor_before = fixture
        .scope
        .current_physical_attempt(successor.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if successor_before.intent_id != successor.intent_id {
        return Err(HostError::Journal);
    }
    let disposition = fixture
        .scope
        .record_scoped_job_started(
            &fixture.assignment,
            9_301,
            &fixture.source,
            &fixture.runner,
            Some("retired-job"),
        )
        .await?;
    if disposition.active() {
        return Err(HostError::Journal);
    }
    if fixture
        .scope
        .current_physical_attempt(fixture.reservation_id)
        .await?
        .is_some()
    {
        return Err(HostError::Journal);
    }
    let successor_after = fixture
        .scope
        .current_physical_attempt(successor.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    if successor_after.intent_id != successor_before.intent_id {
        return Err(HostError::Journal);
    }
    read_transaction(fixture.scope.journal_ref(), async |connection| {
        super::assert_no_historical_cleanup(connection, fixture.intent_id).await
    })
    .await
}

#[test]
fn late_completed_event_after_retirement_records_no_cleanup() -> Result<(), HostError> {
    super::on_large_test_stack(late_completed_event_after_retirement_records_no_cleanup_inner())
}

async fn late_completed_event_after_retirement_records_no_cleanup_inner() -> Result<(), HostError> {
    let fixture = super::retired_native_fixture().await?;
    assert_retired_disposition(&fixture).await?;
    let started = fixture
        .scope
        .record_scoped_job_started(
            &fixture.assignment,
            9_350,
            &fixture.source,
            &fixture.runner,
            Some("retired-completed-job"),
        )
        .await?;
    let completed = fixture
        .scope
        .record_scoped_job_completed(
            &fixture.assignment,
            9_351,
            &fixture.source,
            Some("Succeeded"),
            Some(&fixture.runner),
            Some("retired-completed-job"),
        )
        .await?;
    if started.active()
        || completed.active()
        || fixture
            .scope
            .current_physical_attempt(fixture.reservation_id)
            .await?
            .is_some()
    {
        return Err(HostError::Journal);
    }
    super::assert_historical_intake_state(&fixture).await?;
    replay_retired_events_after_reopen(fixture).await
}

async fn replay_retired_events_after_reopen(
    fixture: super::super::super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let binding = super::super::super::helpers::binding("native-started-intake", 101)?;
    let database_path = fixture.database_path.clone();
    let request_id = fixture.request_id;
    let intent_id = fixture.intent_id;
    let reservation_id = fixture.reservation_id;
    let set_id = fixture.runner.runner_scale_set_id;
    let runner = fixture.runner.clone();
    let source = fixture.source.clone();
    let NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        scope,
        ..
    } = fixture;
    drop(scope);
    drop(journal);

    let reopened = crate::journal::Journal::open(&database_path).await?;
    let scope = reopened.connection_scope(&binding).await?;
    let assignment = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await?;
    let started = scope
        .record_scoped_job_started(
            &assignment,
            9_350,
            &source,
            &runner,
            Some("retired-completed-job"),
        )
        .await?;
    let completed = scope
        .record_scoped_job_completed(
            &assignment,
            9_351,
            &source,
            Some("Succeeded"),
            Some(&runner),
            Some("retired-completed-job"),
        )
        .await?;
    if started.active()
        || completed.active()
        || scope
            .current_physical_attempt(reservation_id)
            .await?
            .is_some()
    {
        return Err(HostError::Journal);
    }
    super::assert_historical_scope_state(&scope, set_id, request_id, intent_id).await?;
    let _keep_scratch_until_reopen_checks_finish = scratch;
    Ok(())
}

async fn assert_retired_disposition(
    fixture: &super::super::super::NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    let scope = fixture.scope.clone();
    in_transaction(scope.journal_ref(), async |connection| {
        use crate::journal::connection::reservation::post_jit_retirement::{
            EventRetirementStatus, disposition_for_event_in_transaction,
        };

        let retired = disposition_for_event_in_transaction(
            connection,
            &scope,
            fixture.runner.runner_scale_set_id,
            fixture.request_id,
            fixture.runner.id,
            fixture.runner.name.as_str(),
        )
        .await?
        .ok_or(HostError::Journal)?;
        if !matches!(retired.status, EventRetirementStatus::Retired) {
            return Err(HostError::Journal);
        }
        let wrong_runner = disposition_for_event_in_transaction(
            connection,
            &scope,
            fixture.runner.runner_scale_set_id,
            fixture.request_id,
            fixture.runner.id + 1,
            fixture.runner.name.as_str(),
        )
        .await?;
        let wrong_set = disposition_for_event_in_transaction(
            connection,
            &scope,
            fixture.runner.runner_scale_set_id + 1,
            fixture.request_id,
            fixture.runner.id,
            fixture.runner.name.as_str(),
        )
        .await?;
        if wrong_runner.is_some() || wrong_set.is_some() {
            return Err(HostError::Journal);
        }
        Ok(())
    })
    .await
}
