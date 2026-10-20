use super::*;

#[tokio::test]
async fn committed_result_is_preserved_and_helper_cleanup_finishes() -> Result<(), HostError> {
    Box::pin(run_committed_result_recovery('7')).await
}

#[tokio::test]
async fn committed_pair_handoff_and_initializer_removal_survive_restart() -> Result<(), HostError> {
    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    configure_test_capacity(&journal).await?;
    let binding = helpers::binding("initializer-committed-removal", 922)?;
    let scope = helpers::created_scope(&journal, &binding, 1922).await?;
    let setup = reserve_handoff_fixture(&scope, '8').await?;
    let HandoffFixtureSetup {
        assignment,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    } = setup;
    let (committed, _release) = crate::journal::arm_create_commit_gate(
        reservation_id,
        crate::journal::CreateCommitStage::Removed,
    )?;
    let mut provisioning = Box::pin(helpers::initialize_reserved_request(
        &scope,
        assignment.clone(),
        assignment,
        '8',
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    ));
    tokio::select! {
        result = &mut provisioning => {
            result?;
            return Err(HostError::Identity);
        }
        result = tokio::time::timeout(Duration::from_secs(10), committed) => {
            result.map_err(|_| HostError::DockerTimeout)?.map_err(|_| HostError::Journal)?;
        }
    }
    let phase = helpers::storage_initializer_phase(&journal, reservation_id).await?;
    assert_eq!(phase.remove, "removed");
    drop(provisioning);
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&scratch.file()).await?;
    let scope = reopened.connection_scope(&binding).await?;
    assert!(
        scope
            .metadata_initialization_ready(reservation_id)
            .await?
            .is_some(),
        "The cleanup commit must leave the runner pair ready after restart."
    );
    assert_eq!(reopened.occupied_reservation_count().await?, 1);
    Ok(())
}

struct HandoffFixtureSetup {
    assignment: crate::journal::AssignmentIdentity,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
}

async fn reserve_handoff_fixture(
    scope: &crate::journal::ConnectionJournal,
    nonce: char,
) -> Result<HandoffFixtureSetup, HostError> {
    use crate::journal::{
        AssignedAcquireResult, AssignmentKey, RequestPhysicalReservationResult, ReservationRequest,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    let sample = format!("{:032x}", nonce as u32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, helpers::TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let request = helpers::acquired_physical_fixture_request(scope).await?;
    let reservation = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &assignment,
                engine_id: helpers::TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: 8_589_934_592,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
            &request,
        )
        .await?;
    let RequestPhysicalReservationResult::AcquireClaimed {
        reservation_id,
        intent_id: acquired_intent_id,
    } = reservation
    else {
        return Err(HostError::Journal);
    };
    scope
        .record_request_acquire(
            &request,
            AssignedAcquireResult::Returned(vec![request.request_id().ok_or(HostError::Identity)?]),
        )
        .await?;
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh || intent_id != acquired_intent_id {
        return Err(HostError::Journal);
    }
    Ok(HandoffFixtureSetup {
        assignment,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    })
}

async fn configure_test_capacity(journal: &Journal) -> Result<(), HostError> {
    journal
        .configure_capacity(CapacityLimits {
            jobs: 1,
            cpu_millicores: 2_000,
            memory_bytes: 25_769_803_776,
        })
        .await
}

async fn run_committed_result_recovery(nonce: char) -> Result<(), HostError> {
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
    let volume_identity = test_support::volume_identity(&scope, reservation_id).await?;
    let api = InitializationDockerApi::open(volume_identity, &home, &work, image.runtime_id())?;
    let (committed, release) = crate::journal::arm_create_commit_gate(
        reservation_id,
        crate::journal::CreateCommitStage::Result,
    )?;
    let docker = api.docker.clone();
    let limits = StorageInitializationLimits::from_reserved_budget(
        test_resource_budget()?,
        100,
        128 * 1024 * 1024,
        16,
    )?;
    let metadata = test_support::metadata_object(&scope, reservation_id, nonce).await?;
    let mut initialize = tokio::spawn(async move {
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
    assert_eq!(phase.result, "confirmed");

    let initialize_result = await_committed_initialize(&mut initialize).await;
    drop(release);
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
    assert_helper_deleted(api).await?;
    Ok(())
}

async fn await_committed_initialize<T>(
    initialize: &mut tokio::task::JoinHandle<Result<T, HostError>>,
) -> Result<T, HostError> {
    if let Ok(join_result) = tokio::time::timeout(Duration::from_secs(5), &mut *initialize).await {
        return join_result.map_err(|_| HostError::Journal)?;
    }
    initialize.abort();
    let join_result = initialize.await;
    if !join_result.is_err_and(|error| error.is_cancelled()) {
        return Err(HostError::Journal);
    }
    Err(HostError::DockerTimeout)
}

async fn assert_helper_deleted(api: InitializationDockerApi) -> Result<(), HostError> {
    let capture = api.finish().await?;
    assert!(
        capture
            .requests
            .iter()
            .any(|request| { request == &format!("DELETE /containers/{HELPER_ID}") })
    );
    Ok(())
}
