use super::*;

#[tokio::test]
async fn timeout_api_denies_exact_started_request_and_keeps_charge() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    fixture.record_started().await?;
    let NativeStartedParts {
        scratch,
        journal,
        scope,
        request_id,
        channel,
        ..
    } = NativeStartedParts::from(fixture);

    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id)
            .await?,
        StartedRequestTimeoutResult::Enqueued
    );
    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id)
            .await?,
        StartedRequestTimeoutResult::AlreadyEnqueued
    );
    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id + 1)
            .await?,
        StartedRequestTimeoutResult::NotEligible
    );

    let connection = journal.connection().await?;
    assert_timeout_state(&connection, request_id, true).await?;
    drop(connection);
    drop(scope);
    drop(journal);

    // Opening a journal after timeout must preserve the denied proof and its
    // cleanup charge. It must not turn the timeout into completion or release.
    let reopened = crate::journal::Journal::open(&scratch.file()).await?;
    let connection = reopened.connection().await?;
    assert_timeout_state(&connection, request_id, true).await?;
    drop(connection);
    Ok(())
}

#[tokio::test]
async fn timeout_api_loses_to_a_real_completed_event() -> Result<(), HostError> {
    let fixture = crate::journal::request_lifecycle::tests::native_started_intake_fixture().await?;
    fixture.record_started().await?;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        journal,
        scope,
        request_id,
        message_id,
        assignment,
        runner,
        source,
        channel,
        ..
    } = fixture;
    scope
        .record_job_completed(
            &assignment,
            message_id + 1,
            &source,
            Some("success"),
            Some(&runner),
            Some("opaque-started-job"),
        )
        .await?;

    assert_eq!(
        scope
            .deny_started_request_timeout(&channel, request_id)
            .await?,
        StartedRequestTimeoutResult::NotEligible
    );
    let connection = journal.connection().await?;
    assert_completed_state(&connection, request_id).await?;
    Ok(())
}
