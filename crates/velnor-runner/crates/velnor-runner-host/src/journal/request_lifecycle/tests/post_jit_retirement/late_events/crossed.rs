use crate::error::HostError;

#[test]
fn distinct_actual_request_and_source_are_checked_before_late_recording() -> Result<(), HostError> {
    super::on_large_test_stack(
        distinct_actual_request_and_source_are_checked_before_late_recording_inner(),
    )
}

async fn distinct_actual_request_and_source_are_checked_before_late_recording_inner()
-> Result<(), HostError> {
    let fixture = super::claimed_native_fixture().await?;
    let actual_id = 8_100_001;
    super::super::helpers::record_acquired(&fixture.scope, actual_id).await?;
    let actual = fixture
        .scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest {
            request_id: actual_id,
        })
        .await?;
    let request_id = actual.request_id().ok_or(HostError::Identity)?;
    let source = super::super::helpers::event_source(101, request_id + 1000);
    let wrong_source = super::super::helpers::event_source(101, request_id + 1001);
    assert!(
        fixture
            .scope
            .record_scoped_job_started(
                &actual,
                9_401,
                &wrong_source,
                &fixture.runner,
                Some("crossed-job"),
            )
            .await
            .is_err()
    );
    let wrong_runner = velnor_runner_github::RunnerReference {
        id: fixture.runner.id + 1,
        name: format!("{}-wrong", fixture.runner.name),
        runner_scale_set_id: fixture.runner.runner_scale_set_id,
    };
    assert!(
        fixture
            .scope
            .record_scoped_job_started(&actual, 9_401, &source, &wrong_runner, Some("crossed-job"),)
            .await
            .is_err()
    );
    let recorded = fixture
        .scope
        .record_scoped_job_started(
            &actual,
            9_401,
            &source,
            &fixture.runner,
            Some("crossed-job"),
        )
        .await?;
    if recorded.active() {
        return Err(HostError::Journal);
    }
    super::assert_actual_request_owns_event(&fixture, actual_id).await
}
