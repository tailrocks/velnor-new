#[tokio::test]
async fn authorized_runnerless_completion_survives_restart_and_blocks_acquire()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 92 })
        .await?;
    let source_receipt = receipt(&scope, 92, 1_092, 101).await?;
    scope
        .record_source_authorization(&request, &source_receipt)
        .await?;
    let event_source = event_source(101, 1_092);
    assert_eq!(
        scope
            .record_job_completed(
                &request,
                903,
                &event_source,
                Some("cancelled"),
                None,
                Some("opaque-job"),
            )
            .await?,
        None
    );
    assert!(!scope.claim_request_acquire(&request).await?);
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 92 })
        .await?;
    assert_eq!(
        scope
            .record_job_completed(
                &request,
                903,
                &event_source,
                Some("cancelled"),
                None,
                Some("opaque-job"),
            )
            .await?,
        None
    );
    assert!(!scope.claim_request_acquire(&request).await?);
    Ok(())
}
