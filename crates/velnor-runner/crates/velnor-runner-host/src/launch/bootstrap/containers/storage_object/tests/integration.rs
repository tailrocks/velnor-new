use super::*;

#[cfg(unix)]
#[tokio::test]
async fn typed_volume_create_uses_journal_claim_and_inspected_docker_effect() -> Result<(), String>
{
    let fixture = physical_fixture("typed-volume-create").await?;
    let names = WorkerIdentityNames::from_nonce(NONCE).map_err(|error| error.to_string())?;
    let name = names.volume_name(crate::worker::WorkerVolumeRole::Home);
    let labels = HashMap::from([
        ("velnor.connection".to_owned(), "object-producer".to_owned()),
        (
            "velnor.reservation".to_owned(),
            fixture.reservation_id.to_string(),
        ),
        ("velnor.operation".to_owned(), fixture.intent_id.to_string()),
        (
            "velnor.role".to_owned(),
            StorageRole::RunnerHome.as_str().to_owned(),
        ),
    ]);
    let volume = local_volume(&name, &labels);
    let image_runner = image_inspect("velnor-runner", &fixture.images, fixture.images.runner());
    let image_dind = image_inspect("velnor-dind", &fixture.images, fixture.images.dind());
    let stub = crate::launch::inspect_tests::DockerStub::open(vec![
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(404, r#"{"message":"no such volume"}"#),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(201, &volume),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &volume),
        crate::launch::inspect_tests::http(200, &daemon_info()),
    ])?;

    let proof = create_storage_object(
        &stub.docker,
        &fixture.scope,
        fixture.reservation_id,
        StorageRole::RunnerHome,
        &fixture.selected,
        &fixture.images,
        &fixture.observation,
    )
    .await
    .map_err(|error| error.to_string())?;
    let requests = stub.finish().await?;

    assert_eq!(proof.reservation_id(), fixture.reservation_id);
    assert_eq!(proof.operation_id(), fixture.intent_id);
    assert_eq!(proof.docker_name(), name);
    assert_eq!(proof.docker_id(), name);
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("POST ") && request.contains("/volumes/create")),
        "unexpected Docker requests: {requests:?}"
    );
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::RunnerHome)
            .await
            .map_err(|error| error.to_string())?
            .is_none()
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unrelated_fresh_sample_cannot_create_object_for_original_reservation() -> Result<(), String>
{
    let fixture = physical_fixture("typed-volume-recovery-sample").await?;
    let recovery_permit = StartPermit::test_fixture_with_engine_id(&"b".repeat(32), ENGINE_ID);
    let observation = recovery_permit.observation_witness();
    assert_ne!(observation.sample_generation_id(), &"a".repeat(32));
    assert!(observation.is_fresh_for(
        ENGINE_ID,
        fixture.selected.docker_root_digest(),
        observation.sample_generation_id(),
    ));

    let mut responses = Vec::new();
    append_verified_engine_responses(&mut responses, &fixture.images);
    let stub = crate::launch::inspect_tests::DockerStub::open(responses)?;

    let result = create_storage_object(
        &stub.docker,
        &fixture.scope,
        fixture.reservation_id,
        StorageRole::RunnerHome,
        &fixture.selected,
        &fixture.images,
        &observation,
    )
    .await;
    let requests = stub.finish().await?;

    assert!(matches!(result, Err(crate::error::HostError::Identity)));
    assert!(
        !requests
            .iter()
            .any(|request| { request.starts_with("POST ") && request.contains("/volumes/create") })
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn foreign_existing_volume_is_never_adopted_or_replaced() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-foreign").await?;
    let names = WorkerIdentityNames::from_nonce(NONCE).map_err(|error| error.to_string())?;
    let name = names.volume_name(crate::worker::WorkerVolumeRole::Home);
    let foreign_labels = HashMap::from([("owner".to_owned(), "other-job".to_owned())]);
    let foreign_volume = local_volume(&name, &foreign_labels);
    let image_runner = image_inspect("velnor-runner", &fixture.images, fixture.images.runner());
    let image_dind = image_inspect("velnor-dind", &fixture.images, fixture.images.dind());
    let stub = crate::launch::inspect_tests::DockerStub::open(vec![
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &foreign_volume),
    ])?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::RunnerHome,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .is_err()
    );
    let requests = stub.finish().await?;
    assert!(
        !requests
            .iter()
            .any(|request| request.starts_with("POST ") && request.contains("/volumes/create")),
        "producer sent a create request for a foreign volume: {requests:?}"
    );
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::RunnerHome)
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "uncertain volume claim became available again"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn mismatched_observation_cannot_create_a_volume() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-wrong-sample").await?;
    let other_permit = StartPermit::test_fixture_with_engine_id(&"c".repeat(32), ENGINE_ID);
    let other_observation = other_permit.observation_witness();
    let image_runner = image_inspect("velnor-runner", &fixture.images, fixture.images.runner());
    let image_dind = image_inspect("velnor-dind", &fixture.images, fixture.images.dind());
    let stub = crate::launch::inspect_tests::DockerStub::open(vec![
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
    ])?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::RunnerHome,
            &fixture.selected,
            &fixture.images,
            &other_observation,
        )
        .await
        .is_err()
    );
    let requests = stub.finish().await?;
    assert!(
        !requests
            .iter()
            .any(|request| request.contains("/volumes/create")),
        "producer sent a create request with a mismatched observation: {requests:?}"
    );
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::RunnerHome)
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "invalid observation did not hold its claimed capacity"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn engine_metadata_is_rejected_before_docker_or_journal_changes() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-metadata-rejected").await?;
    let stub = crate::launch::inspect_tests::DockerStub::open(Vec::new())?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::EngineMetadata,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .is_err()
    );
    assert_eq!(stub.finish().await?.len(), 0);
    assert!(
        fixture
            .scope
            .claim_storage_object_create(fixture.reservation_id, StorageRole::EngineMetadata)
            .await
            .map_err(|error| error.to_string())?
            .is_some(),
        "metadata claim changed before its separate disk adapter exists"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn lost_create_response_keeps_capacity_after_journal_reopen() -> Result<(), String> {
    let fixture = physical_fixture("typed-volume-lost-response").await?;
    let names = WorkerIdentityNames::from_nonce(NONCE).map_err(|error| error.to_string())?;
    let name = names.volume_name(crate::worker::WorkerVolumeRole::Home);
    let image_runner = image_inspect("velnor-runner", &fixture.images, fixture.images.runner());
    let image_dind = image_inspect("velnor-dind", &fixture.images, fixture.images.dind());
    let stub = crate::launch::inspect_tests::DockerStub::open(vec![
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(200, &image_runner),
        crate::launch::inspect_tests::http(200, &image_dind),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::http(404, r#"{"message":"no such volume"}"#),
        crate::launch::inspect_tests::http(200, &daemon_info()),
        crate::launch::inspect_tests::closed(),
    ])?;

    assert!(
        create_storage_object(
            &stub.docker,
            &fixture.scope,
            fixture.reservation_id,
            StorageRole::RunnerHome,
            &fixture.selected,
            &fixture.images,
            &fixture.observation,
        )
        .await
        .is_err()
    );
    let requests = stub.finish().await?;
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with("POST ") && request.contains("/volumes/create")),
        "Docker did not receive the creation request: {requests:?}"
    );
    assert!(
        requests.iter().any(|request| request.contains(&name)),
        "producer did not inspect the deterministic owned volume: {requests:?}"
    );

    let journal = Journal::open(&fixture.scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let reopened = journal
        .connection_scope(&fixture.connection)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        reopened
            .claim_storage_object_create(fixture.reservation_id, StorageRole::RunnerHome)
            .await
            .map_err(|error| error.to_string())?
            .is_none(),
        "lost Docker response made the claim available after journal reopen"
    );
    Ok(())
}
