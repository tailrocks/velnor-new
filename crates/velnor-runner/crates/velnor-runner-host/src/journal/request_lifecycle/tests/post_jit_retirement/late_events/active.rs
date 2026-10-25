use crate::error::HostError;
use crate::journal::connection::storage::read_transaction;

#[test]
fn late_started_and_completed_events_keep_actual_history_only() -> Result<(), HostError> {
    super::on_large_test_stack(late_started_and_completed_events_keep_actual_history_only_inner())
}

async fn late_started_and_completed_events_keep_actual_history_only_inner() -> Result<(), HostError>
{
    let fixture = super::claimed_native_fixture().await?;
    let started = fixture
        .scope
        .record_scoped_job_started(
            &fixture.assignment,
            9_101,
            &fixture.source,
            &fixture.runner,
            Some("late-job"),
        )
        .await?;
    assert!(!started.active());

    let completed = fixture
        .scope
        .record_scoped_job_completed(
            &fixture.assignment,
            9_102,
            &fixture.source,
            Some("Succeeded"),
            Some(&fixture.runner),
            Some("late-job"),
        )
        .await?;
    assert!(!completed.active());

    let started_replay = fixture
        .scope
        .record_scoped_job_started(
            &fixture.assignment,
            9_101,
            &fixture.source,
            &fixture.runner,
            Some("late-job"),
        )
        .await?;
    let completed_replay = fixture
        .scope
        .record_scoped_job_completed(
            &fixture.assignment,
            9_102,
            &fixture.source,
            Some("Succeeded"),
            Some(&fixture.runner),
            Some("late-job"),
        )
        .await?;
    assert!(!started_replay.active());
    assert!(!completed_replay.active());
    assert!(
        fixture
            .scope
            .record_scoped_job_started(
                &fixture.assignment,
                9_103,
                &fixture.source,
                &fixture.runner,
                Some("late-job"),
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .scope
            .record_scoped_job_completed(
                &fixture.assignment,
                9_102,
                &fixture.source,
                Some("Cancelled"),
                Some(&fixture.runner),
                Some("late-job"),
            )
            .await
            .is_err()
    );
    Ok(())
}

#[test]
fn historical_completion_may_arrive_before_started_and_replay_exactly() -> Result<(), HostError> {
    super::on_large_test_stack(
        historical_completion_may_arrive_before_started_and_replay_exactly_inner(),
    )
}

async fn historical_completion_may_arrive_before_started_and_replay_exactly_inner()
-> Result<(), HostError> {
    let fixture = super::claimed_native_fixture().await?;
    let completed = fixture
        .scope
        .record_scoped_job_completed(
            &fixture.assignment,
            9_151,
            &fixture.source,
            Some("Succeeded"),
            Some(&fixture.runner),
            Some("reversed-job"),
        )
        .await?;
    let started = fixture
        .scope
        .record_scoped_job_started(
            &fixture.assignment,
            9_152,
            &fixture.source,
            &fixture.runner,
            Some("reversed-job"),
        )
        .await?;
    let replay = fixture
        .scope
        .record_scoped_job_started(
            &fixture.assignment,
            9_152,
            &fixture.source,
            &fixture.runner,
            Some("reversed-job"),
        )
        .await?;
    if completed.active() || started.active() || replay.active() {
        return Err(HostError::Journal);
    }
    read_transaction(fixture.scope.journal_ref(), async |connection| {
        super::assert_no_historical_cleanup(connection, fixture.intent_id).await
    })
    .await
}
