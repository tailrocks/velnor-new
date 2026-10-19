#[tokio::test]
async fn started_event_replay_works_before_and_after_completion_and_restart()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let fixture = started_fixture(&journal).await?;
    reject_changed_sources(&fixture).await?;
    record_and_clean_started_runner(&fixture).await?;
    let binding = fixture.binding.clone();
    let physical = fixture.physical.clone();
    let intent_id = fixture.intent_id;
    let request = fixture.request.clone();
    let source = fixture.source.clone();
    let runner = fixture.runner.clone();
    drop(fixture);
    drop(journal);

    assert_started_replay_after_restart(
        &scratch.file(),
        &binding,
        &physical,
        intent_id,
        &request,
        &source,
        &runner,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn absent_event_source_fields_match_authorization_but_presence_is_replay_identity()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let fixture = started_fixture(&journal).await?;
    let absent = SourceMetadata::default();
    assert_absent_start_replay_and_presence(&fixture, &absent).await?;
    assert_absent_completion_replay_and_presence(&fixture, &absent).await?;

    let binding = fixture.binding.clone();
    let physical = fixture.physical.clone();
    let intent_id = fixture.intent_id;
    let request = fixture.request.clone();
    let runner = fixture.runner.clone();
    drop(fixture);
    drop(journal);

    assert_replay_survives_restart(
        &path, &binding, &physical, intent_id, &request, &runner, &absent,
    )
    .await?;
    Ok(())
}

async fn assert_absent_start_replay_and_presence(
    fixture: &StartedFixture,
    absent: &SourceMetadata,
) -> Result<(), HostError> {
    for _ in 0..2 {
        assert_eq!(
            fixture
                .scope
                .record_job_started(&fixture.request, 910, absent, &fixture.runner, None)
                .await?,
            fixture.intent_id
        );
    }
    let present = SourceMetadata {
        owner_name: fixture.source.owner_name.clone(),
        ..SourceMetadata::default()
    };
    assert!(matches!(
        fixture
            .scope
            .record_job_started(&fixture.request, 910, &present, &fixture.runner, None)
            .await,
        Err(HostError::Identity)
    ));
    Ok(())
}

async fn assert_absent_completion_replay_and_presence(
    fixture: &StartedFixture,
    absent: &SourceMetadata,
) -> Result<(), HostError> {
    for _ in 0..2 {
        assert_eq!(
            fixture
                .scope
                .record_job_completed(
                    &fixture.request,
                    911,
                    absent,
                    Some("cancelled"),
                    Some(&fixture.runner),
                    None,
                )
                .await?,
            Some(fixture.intent_id)
        );
    }
    let present = SourceMetadata {
        owner_name: fixture.source.owner_name.clone(),
        ..SourceMetadata::default()
    };
    assert!(matches!(
        fixture
            .scope
            .record_job_completed(
                &fixture.request,
                911,
                &present,
                Some("cancelled"),
                Some(&fixture.runner),
                None,
            )
            .await,
        Err(HostError::Identity)
    ));
    Ok(())
}

async fn assert_replay_survives_restart(
    path: &std::path::Path,
    binding: &crate::config::RepositoryConnection,
    physical: &AssignmentIdentity,
    intent_id: i64,
    request: &AssignmentIdentity,
    runner: &RunnerReference,
    absent: &SourceMetadata,
) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    let scope = journal.connection_scope(binding).await?;
    assert_eq!(
        scope
            .record_job_started(request, 910, absent, runner, None)
            .await?,
        intent_id
    );
    assert_eq!(
        scope
            .record_job_completed(request, 911, absent, Some("cancelled"), Some(runner), None,)
            .await?,
        Some(intent_id)
    );
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}
