use super::*;

#[tokio::test]
async fn dispatched_create_restart_marks_hold_uncertain_and_keeps_it_charged()
-> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let binding = helpers::binding("initializer-dispatch-restart", 923)?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let scope = helpers::created_scope(&journal, &binding, 1923).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, '9').await?;
    let image = specification::test_verified_image()?;
    let claim = scope
        .claim_storage_initialization(reservation_id, &image, helpers::test_initializer_limits()?)
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_storage_initializer_create_dispatched(&claim)
        .await?;
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&scratch.file()).await?;
    let scope = reopened.connection_scope(&binding).await?;
    let names = WorkerIdentityNames::from_nonce(&"9".repeat(32))?;
    let home = names.volume_name(WorkerVolumeRole::Home);
    let work = names.volume_name(WorkerVolumeRole::Work);
    let api = InitializationDockerApi::open_recovery(&home, &work, helpers::TEST_ENGINE_ID, None)?;

    assert_eq!(
        recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(10)).await,
        Err(HostError::Journal),
        "a missing helper does not settle a dispatched create"
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "uncertain"
    );
    let assignment = scope
        .assignment_identity(crate::journal::AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "9".repeat(32),
        })
        .await?;
    let reservation = scope
        .reservation(&assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        reservation.state,
        crate::journal::ReservationState::Uncertain
    );
    let capture = api.finish().await?;
    assert!(capture.requests.contains(&"GET /info".to_owned()));
    assert!(capture.requests.iter().any(|request| {
        request.starts_with("GET /containers/velnor-init-") && request.ends_with("/json")
    }));
    assert!(
        !capture
            .requests
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );
    Ok(())
}

#[tokio::test]
async fn delete_before_absence_commit_survives_journal_reopen() -> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let binding = helpers::binding("initializer-delete-restart", 924)?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let scope = helpers::created_scope(&journal, &binding, 1924).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'a').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    let home = names.volume_name(WorkerVolumeRole::Home);
    let work = names.volume_name(WorkerVolumeRole::Work);
    let api =
        InitializationDockerApi::open_lost_start_held_delete(&home, &work, image.runtime_id())?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'a').await?;
    let initialization_result = initialize(
        &api.docker,
        &scope,
        reservation_id,
        &image,
        limits,
        Duration::from_secs(10),
        &metadata,
    )
    .await;
    assert!(
        initialization_result.is_err(),
        "the start failure must leave the helper for recovery"
    );

    let first_docker = api.docker.clone();
    let first_scope = scope.clone();
    let first_recovery = tokio::spawn(async move {
        recover_absent(
            &first_docker,
            &first_scope,
            reservation_id,
            Duration::from_secs(10),
        )
        .await
    });
    let mut api = api;
    let deletion = tokio::time::timeout(Duration::from_secs(10), api.wait_for_delete()).await;
    assert!(
        deletion.is_ok(),
        "recovery did not reach DELETE; requests: {:?}",
        api.capture.lock().await.requests
    );
    deletion.map_err(|_| HostError::DockerTimeout)??;
    first_recovery.abort();
    assert!(first_recovery.await.is_err());

    drop(scope);
    drop(journal);
    let reopened = Journal::open(&scratch.file()).await?;
    let reopened_scope = reopened.connection_scope(&binding).await?;
    assert_eq!(
        helpers::storage_initializer_abort_state(&reopened_scope, reservation_id).await?,
        "remove_requested",
        "reopen must retain the committed removal intent"
    );
    reopened_scope
        .expire_storage_initializer_abort_lease(reservation_id)
        .await?;

    assert_eq!(
        recover_absent(
            &api.docker,
            &reopened_scope,
            reservation_id,
            Duration::from_secs(10),
        )
        .await,
        Ok(())
    );
    let phase = helpers::storage_initializer_phase(&reopened, reservation_id).await?;
    assert_eq!(phase.abort, "absent");
    let capture = api.finish().await?;
    assert_eq!(
        capture
            .requests
            .iter()
            .filter(|request| request.as_str() == "DELETE /containers/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
            .count(),
        1,
        "the retry must inspect absence without repeating deletion"
    );
    assert_eq!(
        capture
            .requests
            .iter()
            .filter(|request| request.as_str() == "POST /containers/create")
            .count(),
        1,
        "recovery must not replay helper creation"
    );
    assert!(capture.requests.iter().any(|request| {
        request == "GET /containers/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/json"
    }));
    Ok(())
}
