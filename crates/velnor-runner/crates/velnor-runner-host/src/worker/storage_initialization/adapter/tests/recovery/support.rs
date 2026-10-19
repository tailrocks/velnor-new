use super::*;

pub(super) async fn assert_inspect_timeout_keeps_charge(
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

pub(super) async fn assert_initializer_charge(
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

pub(super) async fn run_lost_create(
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
    let volume_identity = test_support::volume_identity(&scope, reservation_id).await?;
    let api = InitializationDockerApi::open_lost_create(
        volume_identity,
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
    assert_eq!(
        journal.occupied_reservation_count().await?,
        1,
        "Cleanup must keep the physical reservation."
    );
    let capture = api.finish().await?;
    Ok((recovered, abort_state, capture.requests))
}

pub(super) async fn run_recovery(
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
    let volume_identity = test_support::volume_identity(&scope, reservation_id).await?;
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
        volume_identity,
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
