use super::*;

#[test]
fn readiness_false_blocks_acquire_while_native_cleanup_releases_completed_work()
-> Result<(), String> {
    run_serialized_diagnostic_test(
        "readiness_false_blocks_acquire_while_native_cleanup_releases_completed_work",
        || {
            Box::pin(async {
                readiness_cleanup_case()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn readiness_cleanup_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    let connection_id = fixture.scope.connection_id().as_str();
    fixture
        .server
        .expose_owned_containers(connection_id)
        .await?;
    fixture.server.expose_owned_volumes(connection_id).await;
    fixture
        .server
        .set_capacity_values(
            19 * 1024 * 1024 * 1024,
            19 * 1024 * 1024 * 1024,
            40 * 1024 * 1024 * 1024,
        )
        .await;
    let mut acquisition_calls = 0;
    let admission = crate::launch::turn::admit_when_ready(false, || async {
        acquisition_calls += 1;
        Ok(crate::launch::turn::PositiveAdmission::NoAcquire)
    })
    .await
    .map_err(|_| HostError::Journal)?;
    assert!(matches!(
        admission,
        crate::launch::turn::PositiveAdmission::NoAcquire
    ));
    assert_eq!(acquisition_calls, 0);

    // Startup can set readiness false when guest storage sampling is absent;
    // CompletionWorker still starts independently in finish_connection_preparation.
    let (launch, now) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, claim.generation, now)
            .await?
    );
    assert!(run_native_cleanup(&fixture, &launch, claim).await?.is_ok());
    assert_eq!(fixture.server.effects().container_deletes, 2);
    assert!(fixture.server.effects().volume_deletes > 0);
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 0);
    Ok(())
}

#[test]
fn cleanup_of_one_journal_does_not_delete_another_identity_on_the_same_engine() -> Result<(), String>
{
    run_serialized_diagnostic_test(
        "cleanup_of_one_journal_does_not_delete_another_identity_on_the_same_engine",
        || {
            Box::pin(async {
                Box::pin(cross_journal_cleanup_case())
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn cross_journal_cleanup_case() -> Result<(), HostError> {
    use crate::launch::bootstrap::cleanup_runtime_fixture_for_base_with_capacity;

    let (first, second, _first_identity, second_identity) = cross_journal_fixtures().await?;

    let fixture = cleanup_runtime_fixture_for_base_with_capacity(
        first,
        19 * 1024 * 1024 * 1024,
        19 * 1024 * 1024 * 1024,
        40 * 1024 * 1024 * 1024,
    )
    .await?;
    let connection_id = fixture.scope.connection_id().as_str();
    fixture
        .server
        .expose_owned_containers(connection_id)
        .await?;
    fixture.server.expose_owned_volumes(connection_id).await;
    fixture
        .server
        .expose_secondary_owned_identity(
            second.scope.connection_id().as_str(),
            second_identity.clone(),
        )
        .await;
    let docker = fixture.server.docker();
    assert_identity_inspectable(&docker, &second_identity).await?;
    let (launch, now) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, claim.generation, now)
            .await?
    );
    let cleanup_result = run_native_cleanup(&fixture, &launch, claim).await?;
    assert!(
        cleanup_result.is_ok(),
        "native cleanup failed: {cleanup_result:?}"
    );

    let container_deletes = fixture.server.container_delete_attempts().await;
    let volume_deletes = fixture.server.volume_delete_attempts().await;
    assert!(!container_deletes.iter().any(|(id, _)| {
        id == second_identity.runner_container_id() || id == second_identity.dind_container_id()
    }));
    assert!(!volume_deletes.iter().any(|(name, _)| {
        second_identity
            .volumes()
            .iter()
            .any(|(_, other)| other == name)
    }));
    assert_eq!(second.journal.occupied_reservation_count().await?, 1);
    assert!(
        second
            .scope
            .storage_cleanup_identity(second.reservation_id)
            .await?
            .is_some()
    );

    assert_identity_inspectable(&docker, &second_identity).await?;
    Ok(())
}

async fn assert_identity_inspectable(
    docker: &bollard::Docker,
    identity: &crate::journal::StorageCleanupIdentity,
) -> Result<(), HostError> {
    for id in [identity.runner_container_id(), identity.dind_container_id()] {
        docker
            .inspect_container(id, None)
            .await
            .map_err(|_| HostError::Docker)?;
    }
    for (_, name) in identity.volumes() {
        docker
            .inspect_volume(name)
            .await
            .map_err(|_| HostError::Docker)?;
    }
    Ok(())
}

async fn cross_journal_fixtures() -> Result<
    (
        crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
        crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture,
        crate::journal::StorageCleanupIdentity,
        crate::journal::StorageCleanupIdentity,
    ),
    HostError,
> {
    use crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture;

    let filesystem_instance_digest =
        NativeStartedIntakeFixture::fake_runtime_filesystem_instance_digest();
    let first = NativeStartedIntakeFixture::with_identity_and_filesystem_digest(
        "cleanup-journal-a",
        101,
        501,
        201,
        1_501,
        801,
        'a',
        filesystem_instance_digest.clone(),
    )
    .await?;
    let second = NativeStartedIntakeFixture::with_identity_and_filesystem_digest(
        "cleanup-journal-b",
        101,
        501,
        201,
        1_501,
        802,
        'b',
        filesystem_instance_digest,
    )
    .await?;
    let first_identity = first
        .scope
        .storage_cleanup_identity(first.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let second_identity = second
        .scope
        .storage_cleanup_identity(second.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    assert_eq!(first_identity.engine_id(), second_identity.engine_id());
    assert_eq!(
        first_identity.reservation_id(),
        second_identity.reservation_id()
    );
    assert_eq!(
        first_identity.operation_id(),
        second_identity.operation_id()
    );
    assert_ne!(
        first_identity.runner_container_id(),
        second_identity.runner_container_id()
    );
    assert_ne!(
        first_identity.dind_container_id(),
        second_identity.dind_container_id()
    );
    assert_ne!(
        first_identity.private_volume(),
        second_identity.private_volume()
    );
    Ok((first, second, first_identity, second_identity))
}
