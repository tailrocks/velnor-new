use super::*;

#[tokio::test]
async fn an_unacquired_authorized_request_cannot_start_a_runner() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let (physical, intent_id) = physical_intent(&scope, 'a').await?;
    let reservation_id = scope
        .reservation(&physical)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    let (generation, digest) = scope.test_active_authority_identity().await?;

    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                reservation_id,
                45,
                &runner_name('a'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );

    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 45 })
        .await?;
    let authorized = receipt(&scope, 45, 89, 101).await?;
    scope
        .record_source_authorization(&request, &authorized)
        .await?;

    assert!(
        scope
            .claim_runner_jit_for_request(
                intent_id,
                reservation_id,
                45,
                &runner_name('a'),
                generation,
                &digest
            )
            .await?
            .is_none()
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}

#[tokio::test]
async fn completion_for_request_b_does_not_release_runner_a_and_cancel_is_request_only()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 61).await?;
    let cancel = acquire_request_for_cancellation(&scope, 63).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 62, 'd', 'd').await?;
    let provisioned = initialized.provisioned.clone();
    let physical = provisioned.assignment.clone();
    let intent_id = provisioned.intent_id;
    let runner = RunnerReference {
        id: 761,
        name: runner_name('d'),
        runner_scale_set_id: 501,
    };
    finish_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 62).await?;

    assert_eq!(
        scope
            .record_job_completed(
                &cancel,
                263,
                &event_source(101, 1063),
                Some("cancelled"),
                None,
                Some("opaque"),
            )
            .await?,
        None
    );
    let request_b = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 62 })
        .await?;
    assert_eq!(
        scope
            .record_job_completed(
                &request_b,
                264,
                &event_source(101, 1062),
                Some("success"),
                Some(&runner),
                Some("opaque-job-id"),
            )
            .await?,
        Some(intent_id)
    );
    assert_eq!(
        scope
            .record_job_started(
                &request_b,
                265,
                &event_source(101, 1062),
                &runner,
                Some("opaque-job-id"),
            )
            .await?,
        intent_id
    );
    assert!(scope.intent(intent_id).await?.runner_completed);
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    Ok(())
}
