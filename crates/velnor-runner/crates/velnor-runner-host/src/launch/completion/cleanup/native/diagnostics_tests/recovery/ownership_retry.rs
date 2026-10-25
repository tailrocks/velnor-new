use super::*;

#[test]
fn native_dind_delete_retry_preserves_the_charge_until_cleanup_settles() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "native_dind_delete_retry_preserves_the_charge_until_cleanup_settles",
        || {
            Box::pin(async {
                dind_delete_retry_case()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn dind_delete_retry_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    let identity = fixture
        .scope
        .storage_cleanup_identity(fixture.reservation_id)
        .await?
        .ok_or(HostError::Ownership)?;
    let runner_id = identity.runner_container_id().to_owned();
    let dind_id = identity.dind_container_id().to_owned();
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    fixture
        .server
        .set_capacity_values(
            19 * 1024 * 1024 * 1024,
            19 * 1024 * 1024 * 1024,
            40 * 1024 * 1024 * 1024,
        )
        .await;
    fixture.server.fail_container_delete_once(&dind_id).await;
    let (launch, now) = seed_completion(&fixture).await?;
    let first = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, first.generation, now)
            .await?
    );

    let first_result = run_native_cleanup(&fixture, &launch, first).await?;
    assert!(first_result.is_err());
    assert_eq!(
        owned_delete_attempts(&fixture, &runner_id, &dind_id).await,
        vec![(runner_id.clone(), 204), (dind_id.clone(), 500)]
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    assert!(
        !fixture
            .scope
            .native_completion_reservation(&launch)
            .await?
            .ok_or(HostError::Journal)?
            .storage_cleanup_proven()
    );

    assert!(
        fixture
            .scope
            .retry_completion_cleanup_at(launch.intent.id, first.generation, now, now + 1)
            .await?
    );
    let retry = fixture
        .scope
        .claim_completion_cleanup_at(launch.intent.id, now + 1, 90)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        fixture
            .scope
            .record_completion_runner_absent_at(launch.intent.id, retry.generation, now + 1)
            .await?
    );
    let retry_result = run_native_cleanup(&fixture, &launch, retry).await?;
    assert!(retry_result.is_ok());

    let attempts = owned_delete_attempts(&fixture, &runner_id, &dind_id).await;
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[2], (dind_id, 204));
    assert_eq!(
        attempts.iter().filter(|(id, _)| id == &runner_id).count(),
        1
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 0);
    Ok(())
}

async fn owned_delete_attempts(
    fixture: &crate::launch::bootstrap::CleanupRuntimeFixture,
    runner_id: &str,
    dind_id: &str,
) -> Vec<(String, u16)> {
    fixture
        .server
        .container_delete_attempts()
        .await
        .into_iter()
        .filter(|(id, _)| id == runner_id || id == dind_id)
        .collect()
}

#[test]
fn foreign_same_name_container_identity_is_not_deleted_or_released() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "foreign_same_name_container_identity_is_not_deleted_or_released",
        || {
            Box::pin(async {
                foreign_container_case()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn foreign_container_case() -> Result<(), HostError> {
    use crate::launch::bootstrap::OwnedContainerMismatch;

    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers_with_mismatch(
            fixture.scope.connection_id().as_str(),
            OwnedContainerMismatch::Id,
        )
        .await?;
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

    let result = run_native_cleanup(&fixture, &launch, claim).await?;
    assert!(result.is_err());
    assert_eq!(
        fixture.server.container_delete_attempts().await,
        [] as [(String, u16); 0]
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    assert!(
        !fixture
            .scope
            .native_completion_reservation(&launch)
            .await?
            .ok_or(HostError::Journal)?
            .storage_cleanup_proven()
    );
    Ok(())
}

#[test]
fn foreign_volume_connection_label_blocks_delete_and_keeps_charge() -> Result<(), String> {
    run_serialized_diagnostic_test(
        "foreign_volume_connection_label_blocks_delete_and_keeps_charge",
        || {
            Box::pin(async {
                foreign_volume_label_case()
                    .await
                    .map_err(|error| error.to_string())
            })
        },
    )
}

async fn foreign_volume_label_case() -> Result<(), HostError> {
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    let connection_id = fixture.scope.connection_id().as_str();
    fixture
        .server
        .expose_owned_containers(connection_id)
        .await?;
    fixture
        .server
        .reject_owned_volume_connection_label(connection_id)
        .await;
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

    assert!(run_native_cleanup(&fixture, &launch, claim).await?.is_err());
    assert_eq!(
        fixture.server.volume_delete_attempts().await,
        [] as [(String, u16); 0]
    );
    assert_eq!(fixture.journal.occupied_reservation_count().await?, 1);
    assert!(
        !fixture
            .scope
            .native_completion_reservation(&launch)
            .await?
            .ok_or(HostError::Journal)?
            .storage_cleanup_proven()
    );
    Ok(())
}

pub(super) async fn native_home_volume_post_delete_inspection_failure_case() -> Result<(), HostError>
{
    let fixture = Box::pin(capacity_retry_fixture()).await?;
    fixture
        .server
        .expose_owned_containers(fixture.scope.connection_id().as_str())
        .await?;
    fixture
        .server
        .fail_owned_volume_inspection_after_delete(fixture.scope.connection_id().as_str())
        .await?;
    let (launch, _) = seed_completion(&fixture).await?;
    let claim = fixture
        .scope
        .claim_completion_cleanup(launch.intent.id, 90)
        .await?
        .ok_or(HostError::Journal)?;
    let diagnostics = super::diagnostics_root(&fixture)?;
    let result = Box::pin(super::run_native_cleanup_with_diagnostics(
        &fixture,
        &launch,
        claim,
        &diagnostics,
    ))
    .await?;

    assert!(matches!(
        result,
        Err(crate::launch::completion::cleanup::Failure {
            phase: "volume absence",
            error: HostError::Docker,
            ..
        })
    ));
    assert!(
        fixture
            .server
            .post_delete_volume_inspection_failure_was_observed()
            .await
    );
    let current = fixture
        .scope
        .native_completion_reservation(&launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(!current.storage_cleanup_proven());

    reopen_post_delete_failure(fixture, launch).await
}

async fn reopen_post_delete_failure(
    fixture: crate::launch::bootstrap::CleanupRuntimeFixture,
    launch: crate::journal::CompletedLaunch,
) -> Result<(), HostError> {
    let crate::launch::bootstrap::CleanupRuntimeFixture {
        base,
        runtime,
        server,
        scope,
        journal,
        ..
    } = fixture;
    let crate::journal::request_lifecycle::tests::NativeStartedIntakeFixture {
        _scratch: scratch,
        journal: base_journal,
        database_path,
        scope: base_scope,
        reservation_assignment,
        provisioned,
        ..
    } = base;
    drop(runtime);
    drop(server);
    drop(scope);
    drop(journal);
    drop(base_scope);
    drop(base_journal);
    drop(provisioned);
    assert_unproven_cleanup_after_reopen(database_path, reservation_assignment, launch).await?;
    drop(scratch);
    Ok(())
}

async fn assert_unproven_cleanup_after_reopen(
    database_path: std::path::PathBuf,
    reservation_assignment: crate::journal::AssignmentIdentity,
    launch: crate::journal::CompletedLaunch,
) -> Result<(), HostError> {
    let reopened = crate::journal::Journal::open(&database_path).await?;
    let binding =
        crate::journal::request_lifecycle::tests::helpers::binding("native-started-intake", 101)?;
    let reopened_scope = reopened.connection_scope(&binding).await?;
    assert_eq!(reopened.occupied_reservation_count().await?, 1);
    let reservation = reopened_scope
        .reservation(&reservation_assignment)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        reservation.state,
        crate::journal::ReservationState::Reserved
    );
    let persisted = reopened_scope
        .native_completion_reservation(&launch)
        .await?
        .ok_or(HostError::Journal)?;
    assert!(!persisted.storage_cleanup_proven());
    drop(reopened_scope);
    drop(reopened);
    Ok(())
}
