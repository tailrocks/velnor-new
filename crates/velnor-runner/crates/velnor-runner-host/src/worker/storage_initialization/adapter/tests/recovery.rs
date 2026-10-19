use super::*;

#[tokio::test]
async fn recovery_rejects_wrong_engine_before_resource_mutation() -> Result<(), HostError> {
    let (wrong_engine, wrong_engine_state, wrong_engine_requests) =
        Box::pin(run_recovery("22222222-2222-4222-8222-222222222222", None)).await?;
    assert_eq!(wrong_engine, Err(HostError::Ownership));
    assert_eq!(wrong_engine_state, "not_requested");
    assert_eq!(wrong_engine_requests, ["GET /info"]);
    Ok(())
}

#[tokio::test]
async fn absent_recovery_requires_selected_engine_and_definitive_not_found() -> Result<(), HostError>
{
    let (absent, absent_state, absent_requests) =
        Box::pin(run_recovery(helpers::TEST_ENGINE_ID, None)).await?;
    assert_eq!(absent, Ok(()), "requests: {absent_requests:?}");
    assert_eq!(absent_state, "absent");
    assert!(absent_requests.iter().any(|request| request == "GET /info"));
    assert!(absent_requests.iter().any(|request| {
        request.starts_with("GET /containers/velnor-init-") && request.ends_with("/json")
    }));
    assert!(
        !absent_requests
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );

    let (failed_inspect, failed_state, failed_requests) =
        Box::pin(run_recovery(helpers::TEST_ENGINE_ID, Some(500))).await?;
    assert_eq!(failed_inspect, Err(HostError::Docker));
    assert_eq!(failed_state, "uncertain");
    assert!(failed_requests.iter().any(|request| {
        request.starts_with("GET /containers/velnor-init-") && request.ends_with("/json")
    }));
    assert!(
        !failed_requests
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );
    Ok(())
}

#[tokio::test]
async fn precreate_validation_failure_holds_capacity_until_absence_recovery()
-> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-precreate", 917)?;
    let scope = helpers::created_scope(&journal, &binding, 1917).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'f').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"f".repeat(32))?;
    let home_volume = names.volume_name(WorkerVolumeRole::Home);
    let work_volume = names.volume_name(WorkerVolumeRole::Work);
    let api = InitializationDockerApi::open(
        &home_volume,
        &work_volume,
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    )?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'f').await?;
    let result = initialize(
        &api.docker,
        &scope,
        reservation_id,
        &image,
        limits,
        Duration::from_secs(10),
        &metadata,
    )
    .await;
    assert_eq!(
        result.err(),
        Some(HostError::Ownership),
        "requests: {:?}",
        api.capture.lock().await.requests
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "not_requested"
    );
    assert_eq!(
        recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(10)).await,
        Ok(())
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "absent"
    );
    let capture = api.finish().await?;
    assert!(capture.requests.iter().any(|request| {
        request.starts_with("GET /containers/velnor-init-") && request.ends_with("/json")
    }));
    assert!(
        !capture
            .requests
            .iter()
            .any(|request| request == "POST /containers/create")
    );
    assert!(
        !capture
            .requests
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );
    Ok(())
}

#[tokio::test]
async fn inspect_timeout_keeps_owned_initializer_charged_until_recovery() -> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-inspect-timeout", 922)?;
    let scope = helpers::created_scope(&journal, &binding, 1922).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'd').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"d".repeat(32))?;
    let home = names.volume_name(WorkerVolumeRole::Home);
    let work = names.volume_name(WorkerVolumeRole::Work);
    let mut api =
        InitializationDockerApi::open_lost_create_held_inspect(&home, &work, image.runtime_id())?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'd').await?;
    assert_eq!(
        initialize(
            &api.docker,
            &scope,
            reservation_id,
            &image,
            limits,
            Duration::from_secs(10),
            &metadata,
        )
        .await
        .err(),
        Some(HostError::ContainerCreateUncertain)
    );
    assert_initializer_charge(&journal, &scope, reservation_id, 'd').await?;

    assert_inspect_timeout_keeps_charge(&journal, &mut api, &scope, reservation_id, 'd').await?;
    assert_initializer_charge(&journal, &scope, reservation_id, 'd').await?;
    let capture = api.finish().await?;
    assert!(
        capture
            .requests
            .iter()
            .any(|request| { request == &format!("DELETE /containers/{HELPER_ID}") })
    );
    assert!(
        !capture
            .requests
            .iter()
            .any(|request| request.ends_with("/start"))
    );
    Ok(())
}

async fn assert_inspect_timeout_keeps_charge(
    journal: &Journal,
    api: &mut InitializationDockerApi,
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    nonce: char,
) -> Result<(), HostError> {
    let docker = api.docker.clone();
    let mut timed_recovery = Box::pin(recover_absent(
        &docker,
        scope,
        reservation_id,
        Duration::from_secs(2),
    ));
    tokio::select! {
        _ = api.wait_for_inspect() => {}
        _result = &mut timed_recovery => {
            return Err(HostError::DockerTimeout);
        }
    }
    assert_eq!(timed_recovery.await, Err(HostError::DockerTimeout));
    assert_initializer_charge(journal, scope, reservation_id, nonce).await?;
    assert_eq!(
        helpers::storage_initializer_abort_state(scope, reservation_id).await?,
        "uncertain"
    );

    api.release_inspect_response();
    tokio::task::yield_now().await;
    assert_eq!(
        recover_absent(&api.docker, scope, reservation_id, Duration::from_secs(10)).await,
        Ok(())
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(scope, reservation_id).await?,
        "absent"
    );
    assert_initializer_charge(journal, scope, reservation_id, nonce).await?;
    Ok(())
}

async fn assert_initializer_charge(
    journal: &Journal,
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    nonce: char,
) -> Result<(), HostError> {
    let assignment = scope
        .assignment_identity(crate::journal::AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    let reservation = scope
        .reservation(&assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(reservation.reservation_id, reservation_id);
    assert_ne!(
        reservation.state,
        crate::journal::ReservationState::Released
    );
    assert_eq!(journal.occupied_reservation_count().await?, 1);
    Ok(())
}

#[tokio::test]
async fn lost_create_response_removes_only_a_fully_verified_owned_helper() -> Result<(), HostError>
{
    let (owned, owned_state, owned_requests) = Box::pin(run_lost_create(false, None)).await?;
    assert_eq!(owned, Ok(()));
    assert_eq!(owned_state, "absent");
    assert!(
        owned_requests
            .iter()
            .any(|request| request == &format!("DELETE /containers/{HELPER_ID}"))
    );

    let (foreign, foreign_state, foreign_requests) = Box::pin(run_lost_create(true, None)).await?;
    assert_eq!(foreign, Err(HostError::Ownership));
    assert_eq!(foreign_state, "uncertain");
    assert!(
        !foreign_requests
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );

    let (failed_remove, remove_state, remove_requests) =
        Box::pin(run_lost_create(false, Some(500))).await?;
    assert_eq!(failed_remove, Err(HostError::Cleanup));
    assert_eq!(remove_state, "uncertain");
    assert!(
        remove_requests
            .iter()
            .any(|request| request == &format!("DELETE /containers/{HELPER_ID}"))
    );
    Ok(())
}

#[tokio::test]
async fn recovery_does_not_remove_a_replacement_for_a_persisted_helper_id() -> Result<(), HostError>
{
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-helper-id-replacement", 920)?;
    let scope = helpers::created_scope(&journal, &binding, 1920).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'b').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"b".repeat(32))?;
    let home = names.volume_name(WorkerVolumeRole::Home);
    let work = names.volume_name(WorkerVolumeRole::Work);
    let replacement_id = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let api = InitializationDockerApi::open_mismatched_helper_id(
        &home,
        &work,
        image.runtime_id(),
        replacement_id,
    )?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'b').await?;
    assert_eq!(
        initialize(
            &api.docker,
            &scope,
            reservation_id,
            &image,
            limits,
            Duration::from_secs(10),
            &metadata,
        )
        .await
        .err(),
        Some(HostError::Ownership)
    );
    assert_eq!(
        recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(10)).await,
        Err(HostError::Ownership)
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "uncertain"
    );
    let capture = api.finish().await?;
    assert!(
        capture
            .requests
            .iter()
            .any(|request| request == "POST /containers/create")
    );
    assert!(
        !capture
            .requests
            .iter()
            .any(|request| request.starts_with("DELETE /containers/"))
    );
    Ok(())
}

#[tokio::test]
async fn recovery_stops_an_uncertain_started_helper_before_removal() -> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-start-recovery", 921)?;
    let scope = helpers::created_scope(&journal, &binding, 1921).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'c').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"c".repeat(32))?;
    let home = names.volume_name(WorkerVolumeRole::Home);
    let work = names.volume_name(WorkerVolumeRole::Work);
    let api = InitializationDockerApi::open_lost_start(&home, &work, image.runtime_id())?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'c').await?;
    assert_eq!(
        initialize(
            &api.docker,
            &scope,
            reservation_id,
            &image,
            limits,
            Duration::from_secs(10),
            &metadata,
        )
        .await
        .err(),
        Some(HostError::ContainerStartUncertain)
    );
    assert_eq!(
        recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(10)).await,
        Ok(())
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "absent"
    );
    let capture = api.finish().await?;
    assert!(
        capture
            .requests
            .iter()
            .any(|request| request == &format!("POST /containers/{HELPER_ID}/stop"))
    );
    assert!(
        capture
            .requests
            .iter()
            .any(|request| request == &format!("DELETE /containers/{HELPER_ID}"))
    );
    assert!(
        capture
            .requests
            .iter()
            .any(|request| request == &format!("GET /containers/{HELPER_ID}/json"))
    );
    assert!(capture.requests.iter().any(|request| {
        capture
            .create_name
            .as_ref()
            .is_some_and(|name| request == &format!("GET /containers/{name}/json"))
    }));
    Ok(())
}

async fn run_lost_create(
    foreign_helper: bool,
    remove_status: Option<u16>,
) -> Result<(Result<(), HostError>, String, Vec<String>), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-lost-create", 918)?;
    let scope = helpers::created_scope(&journal, &binding, 1918).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, '9').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"9".repeat(32))?;
    let home_volume = names.volume_name(WorkerVolumeRole::Home);
    let work_volume = names.volume_name(WorkerVolumeRole::Work);
    let api = InitializationDockerApi::open_lost_create(
        &home_volume,
        &work_volume,
        image.runtime_id(),
        foreign_helper,
        remove_status,
    )?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, '9').await?;
    let initialize_result = initialize(
        &api.docker,
        &scope,
        reservation_id,
        &image,
        limits,
        Duration::from_secs(10),
        &metadata,
    )
    .await;
    assert_eq!(
        initialize_result.err(),
        Some(HostError::ContainerCreateUncertain)
    );
    let recovered =
        recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(10)).await;
    let abort_state = helpers::storage_initializer_abort_state(&scope, reservation_id).await?;
    let capture = api.finish().await?;
    Ok((recovered, abort_state, capture.requests))
}

async fn run_recovery(
    engine_id: &str,
    inspect_status: Option<u16>,
) -> Result<(Result<(), HostError>, String, Vec<String>), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-recovery", 916)?;
    let scope = helpers::created_scope(&journal, &binding, 1916).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'e').await?;
    let image = specification::test_verified_image()?;
    let claim = scope
        .claim_storage_initialization_v2(
            reservation_id,
            &image,
            StorageInitializationLimits::from_reserved_budget(
                test_resource_budget()?,
                100,
                128 * 1024 * 1024,
                16,
            )?,
            &test_support::metadata_object(&scope, reservation_id, 'e').await?,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .mark_storage_initializer_create_uncertain(&claim)
        .await?;
    let names = WorkerIdentityNames::from_nonce(&"e".repeat(32))?;
    let home_volume = names.volume_name(WorkerVolumeRole::Home);
    let work_volume = names.volume_name(WorkerVolumeRole::Work);
    let api = InitializationDockerApi::open_recovery(
        &home_volume,
        &work_volume,
        engine_id,
        inspect_status,
    )?;
    let result = recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(10)).await;
    let capture = api.finish().await?;
    let abort_state = helpers::storage_initializer_abort_state(&scope, reservation_id).await?;
    if engine_id != helpers::TEST_ENGINE_ID {
        assert!(
            !capture
                .requests
                .iter()
                .any(|request| request.starts_with("GET /containers/"))
        );
        assert!(
            !capture
                .requests
                .iter()
                .any(|request| request.starts_with("DELETE /containers/"))
        );
    }
    Ok((result, abort_state, capture.requests))
}
