use super::*;

#[tokio::test]
async fn committed_result_is_preserved_and_helper_cleanup_finishes() -> Result<(), HostError> {
    Box::pin(run_committed_phase_recovery(
        crate::journal::CreateCommitStage::Result,
        '7',
    ))
    .await
}

#[tokio::test]
async fn committed_removal_is_preserved_and_absence_finishes() -> Result<(), HostError> {
    Box::pin(run_committed_phase_recovery(
        crate::journal::CreateCommitStage::Removed,
        '8',
    ))
    .await
}

async fn run_committed_phase_recovery(
    stage: crate::journal::CreateCommitStage,
    nonce: char,
) -> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await?;
    let binding = helpers::binding("initializer-committed-phase", 922)?;
    let scope = helpers::created_scope(&journal, &binding, 1922).await?;
    let reservation_id = helpers::prepare_physical_initializer(&scope, nonce).await?;
    let image = specification::test_verified_image()?;
    let names = WorkerIdentityNames::from_nonce(&nonce.to_string().repeat(32))?;
    let home = names.volume_name(WorkerVolumeRole::Home);
    let work = names.volume_name(WorkerVolumeRole::Work);
    let api = InitializationDockerApi::open(&home, &work, image.runtime_id())?;
    let (committed, _release) = crate::journal::arm_create_commit_gate(reservation_id, stage)?;
    let docker = api.docker.clone();
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, nonce).await?;
    let initialize = tokio::spawn(async move {
        Box::pin(initialize(
            &docker,
            &scope,
            reservation_id,
            &image,
            limits,
            Duration::from_secs(1),
            &metadata,
        ))
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), committed)
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Journal)?;

    // A second connection reads the saved state while the writer waits.
    let observer = Journal::open(&scratch.file()).await?;
    let observer_scope = observer.connection_scope(&binding).await?;
    let phase = helpers::storage_initializer_phase(&observer, reservation_id).await?;
    match stage {
        crate::journal::CreateCommitStage::Result => assert_eq!(phase.result, "confirmed"),
        crate::journal::CreateCommitStage::Removed => {
            assert_eq!(phase.result, "confirmed");
            assert_eq!(phase.remove, "removed");
        }
        _ => return Err(HostError::Identity),
    }

    let initialize_result = tokio::time::timeout(Duration::from_secs(5), initialize)
        .await
        .map_err(|_| HostError::DockerTimeout)?
        .map_err(|_| HostError::Journal)?;
    assert_eq!(initialize_result.err(), Some(HostError::DockerTimeout));
    assert_eq!(
        recover_absent(
            &api.docker,
            &observer_scope,
            reservation_id,
            Duration::from_secs(10)
        )
        .await,
        Ok(())
    );
    let settled = helpers::storage_initializer_phase(&observer, reservation_id).await?;
    assert_eq!(settled.result, "confirmed");
    assert_eq!(settled.abort, "absent");
    if stage == crate::journal::CreateCommitStage::Removed {
        assert_eq!(settled.remove, "removed");
    }
    let capture = api.finish().await?;
    assert!(
        capture
            .requests
            .iter()
            .any(|request| { request == &format!("DELETE /containers/{HELPER_ID}") })
            || stage == crate::journal::CreateCommitStage::Removed
    );
    Ok(())
}
