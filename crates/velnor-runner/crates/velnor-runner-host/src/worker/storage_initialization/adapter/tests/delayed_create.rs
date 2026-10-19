use super::*;

#[tokio::test]
async fn delayed_create_cleans_late_owned_helper_before_response() -> Result<(), HostError> {
    let (_scratch, scope, reservation_id, image, home, work) = delayed_fixture().await?;
    let mut api = InitializationDockerApi::open_held_create(&home, &work, image.runtime_id())?;
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, 'a').await?;
    let docker = api.docker.clone();
    let mut initialize = Box::pin(initialize(
        &docker,
        &scope,
        reservation_id,
        &image,
        limits,
        Duration::from_secs(2),
        &metadata,
    ));
    tokio::select! {
        result = &mut initialize => panic!("Initialization ended before Docker received the create request: {:?}", result.as_ref().err()),
        seen = api.wait_for_create() => seen?,
    }
    tokio::select! {
        result = &mut initialize => assert_eq!(result.err(), Some(HostError::DockerTimeout)),
        () = tokio::time::sleep(Duration::from_secs(3)) => return Err(HostError::DockerTimeout),
    }

    let helper_name = api
        .capture
        .lock()
        .await
        .create_name
        .clone()
        .ok_or(HostError::Identity)?;
    assert_first_absence_keeps_hold(&scope, reservation_id, &home, &work, &helper_name).await?;

    api.release_create();
    api.wait_for_create_response().await?;
    recover_absent(&api.docker, &scope, reservation_id, Duration::from_secs(5)).await?;
    assert_eq!(
        helpers::storage_initializer_abort_state(&scope, reservation_id).await?,
        "absent"
    );
    api.release_create_response();
    api.wait_for_create_completion().await?;

    let capture = api.finish().await?;
    assert_eq!(
        capture
            .requests
            .iter()
            .filter(|request| request.as_str() == "POST /containers/create")
            .count(),
        1
    );
    assert!(
        capture
            .requests
            .iter()
            .any(|request| { request == &format!("DELETE /containers/{HELPER_ID}") })
    );
    assert!(
        capture
            .requests
            .iter()
            .any(|request| { request == &format!("GET /containers/{HELPER_ID}/json") })
    );
    assert!(
        capture
            .requests
            .iter()
            .any(|request| { request == &format!("GET /containers/{helper_name}/json") })
    );
    Ok(())
}

async fn delayed_fixture() -> Result<
    (
        helpers::Scratch,
        crate::journal::ConnectionJournal,
        i64,
        StorageInitializationImage,
        String,
        String,
    ),
    HostError,
> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-delayed-create", 919)?;
    let scope = helpers::created_scope(&journal, &binding, 1919).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, 'a').await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&"a".repeat(32))?;
    Ok((
        scratch,
        scope,
        reservation_id,
        image,
        names.volume_name(WorkerVolumeRole::Home),
        names.volume_name(WorkerVolumeRole::Work),
    ))
}

async fn assert_first_absence_keeps_hold(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    home: &str,
    work: &str,
    helper_name: &str,
) -> Result<(), HostError> {
    let probe = InitializationDockerApi::open_recovery(home, work, helpers::TEST_ENGINE_ID, None)?;
    assert!(matches!(
        probe.docker.inspect_container(helper_name, None).await,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        })
    ));
    assert_eq!(
        recover_absent(&probe.docker, scope, reservation_id, Duration::from_secs(5)).await,
        Err(HostError::Journal)
    );
    assert_eq!(
        helpers::storage_initializer_abort_state(scope, reservation_id).await?,
        "uncertain"
    );
    probe.finish().await?;
    Ok(())
}
