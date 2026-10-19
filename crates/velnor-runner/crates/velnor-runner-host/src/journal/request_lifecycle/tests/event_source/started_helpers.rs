async fn started_fixture(journal: &Journal) -> Result<StartedFixture, HostError> {
    let binding = binding("essential", 101)?;
    let scope = created_scope(journal, &binding, 501).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 91, 'c', 'c').await?;
    let provisioned = initialized.provisioned.clone();
    let physical = provisioned.assignment.clone();
    let intent_id = provisioned.intent_id;
    let runner = RunnerReference {
        id: 791,
        name: runner_name('c'),
        runner_scale_set_id: 501,
    };
    finish_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 91).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 91 })
        .await?;
    Ok(StartedFixture {
        binding,
        scope,
        physical,
        intent_id,
        request,
        source: event_source(101, 1_091),
        runner,
        provisioned,
    })
}

async fn reject_changed_sources(fixture: &StartedFixture) -> Result<(), HostError> {
    for source in changed_sources(&fixture.source)
        .into_iter()
        .chain(invalid_present_sources(&fixture.source))
    {
        assert!(matches!(
            fixture
                .scope
                .record_job_started(
                    &fixture.request,
                    901,
                    &source,
                    &fixture.runner,
                    Some("opaque-job")
                )
                .await,
            Err(HostError::Identity)
        ));
        assert!(matches!(
            fixture
                .scope
                .record_job_completed(
                    &fixture.request,
                    902,
                    &source,
                    Some("success"),
                    Some(&fixture.runner),
                    Some("opaque-job")
                )
                .await,
            Err(HostError::Identity)
        ));
    }
    Ok(())
}

fn changed_sources(
    source: &velnor_runner_github::SourceMetadata,
) -> Vec<velnor_runner_github::SourceMetadata> {
    let mut mismatches = Vec::new();
    let mut changed = source.clone();
    changed.owner_name = Some("other-owner".to_owned());
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.repository_name = Some("other-repository".to_owned());
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.job_workflow_ref = Some("other/workflow.yml@refs/heads/main".to_owned());
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.workflow_run_id = Some(1_092);
    mismatches.push(changed);
    let mut changed = source.clone();
    changed.event_name = Some("workflow_dispatch".to_owned());
    mismatches.push(changed);
    mismatches
}

fn invalid_present_sources(
    source: &velnor_runner_github::SourceMetadata,
) -> Vec<velnor_runner_github::SourceMetadata> {
    let mut invalid = Vec::new();
    let mut changed = source.clone();
    changed.owner_name = Some(String::new());
    invalid.push(changed);
    let mut changed = source.clone();
    changed.repository_name = Some(String::new());
    invalid.push(changed);
    let mut changed = source.clone();
    changed.job_workflow_ref = Some(String::new());
    invalid.push(changed);
    let mut changed = source.clone();
    changed.workflow_run_id = Some(0);
    invalid.push(changed);
    let mut changed = source.clone();
    changed.event_name = Some(String::new());
    invalid.push(changed);
    invalid
}

async fn record_and_clean_started_runner(fixture: &StartedFixture) -> Result<(), HostError> {
    let scope = &fixture.scope;
    assert_started_once(fixture).await?;
    assert_started_once(fixture).await?;
    assert_eq!(
        scope
            .record_job_completed(
                &fixture.request,
                902,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some("opaque-job")
            )
            .await?,
        Some(fixture.intent_id)
    );
    clean_and_release_runner(fixture).await?;
    assert_started_once(fixture).await?;
    Ok(())
}

async fn assert_started_once(fixture: &StartedFixture) -> Result<(), HostError> {
    assert_eq!(
        fixture
            .scope
            .record_job_started(
                &fixture.request,
                901,
                &fixture.source,
                &fixture.runner,
                Some("opaque-job"),
            )
            .await?,
        fixture.intent_id
    );
    Ok(())
}

async fn clean_and_release_runner(fixture: &StartedFixture) -> Result<(), HostError> {
    let scope = &fixture.scope;
    let claim = scope
        .claim_completion_cleanup_at(fixture.intent_id, 0, 10)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        scope
            .run_completion_cleanup_effect_at(fixture.intent_id, claim.generation, 1, 5, || async {
                Ok::<_, HostError>(())
            })
            .await?,
        Some(())
    );
    assert!(
        scope
            .record_completion_runner_absent_at(fixture.intent_id, claim.generation, 2)
            .await?
    );
    assert!(
        scope
            .record_completion_cleanup_at(fixture.intent_id, claim.generation, 3)
            .await?
    );
    let reservation_id = scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?
        .reservation_id;
    record_owned_storage_cleanup(scope, &fixture.provisioned).await?;
    record_owned_network_cleanup(scope, reservation_id).await?;
    scope.record_remote_cleanup(reservation_id).await?;
    scope.record_local_cleanup(reservation_id).await?;
    assert_eq!(
        settle_storage_metadata(scope, &fixture.provisioned, 'd').await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    Ok(())
}

async fn assert_started_replay_after_restart(
    path: &std::path::Path,
    binding: &crate::config::RepositoryConnection,
    physical: &AssignmentIdentity,
    intent_id: i64,
    request: &AssignmentIdentity,
    source: &velnor_runner_github::SourceMetadata,
    runner: &RunnerReference,
) -> Result<(), HostError> {
    let journal = Journal::open(path).await?;
    let scope = journal.connection_scope(binding).await?;
    assert_eq!(
        scope
            .record_job_started(request, 901, source, runner, Some("opaque-job"))
            .await?,
        intent_id
    );
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Released)
    );
    Ok(())
}
