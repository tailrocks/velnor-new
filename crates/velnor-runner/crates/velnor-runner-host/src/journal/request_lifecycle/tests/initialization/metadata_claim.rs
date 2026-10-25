use super::*;
use crate::journal::request_lifecycle::tests::helpers;

#[tokio::test]
async fn metadata_claim_waits_for_its_created_object() -> Result<(), HostError> {
    use crate::journal::{StorageRole, request_lifecycle::tests::helpers::test_initializer_limits};
    use crate::launch::bootstrap::test_created_storage_object;
    use crate::worker::storage_initialization::StorageInitializationImage;
    use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("metadata-claim-pending", 951)?, 1951).await?;
    let (reservation_id, plan, engine_id, root_digest, nonce) =
        prepare_metadata_claim_reservation(&scope, 'b').await?;
    let names = WorkerIdentityNames::from_nonce(&nonce)?;
    for (role, volume_role) in [
        (StorageRole::RunnerHome, WorkerVolumeRole::Home),
        (StorageRole::RunnerWork, WorkerVolumeRole::Work),
    ] {
        let claim = scope
            .claim_storage_object_create(reservation_id, role)
            .await?
            .ok_or(HostError::Journal)?;
        let volume_name = names.volume_name(volume_role);
        if claim.docker_name() != volume_name {
            return Err(HostError::Identity);
        }
        let proof = test_created_storage_object(
            reservation_id,
            plan.operation_id(),
            engine_id.clone(),
            root_digest.clone(),
            role,
            volume_name.clone(),
            volume_name,
        )?;
        scope.record_storage_object_created(&claim, &proof).await?;
    }
    let metadata_name = names.volume_name(WorkerVolumeRole::RunnerMetadata);
    let metadata = test_created_storage_object(
        reservation_id,
        plan.operation_id(),
        engine_id,
        root_digest,
        StorageRole::EngineMetadata,
        metadata_name.clone(),
        metadata_name.clone(),
    )?;
    let before = initializer_fixture_state(&scope, reservation_id, &metadata_name).await?;
    assert_eq!(before.0, 0);
    assert_eq!(before.3, "reserved");
    assert_eq!(before.4, "unclaimed:");
    let images = crate::launch::verified_images::test_worker_images();
    let image = StorageInitializationImage::from_verified_release(&images)?;

    assert!(
        scope
            .claim_storage_initialization_v2(
                reservation_id,
                &image,
                test_initializer_limits()?,
                &metadata
            )
            .await?
            .is_none()
    );
    assert_eq!(
        initializer_fixture_state(&scope, reservation_id, &metadata_name).await?,
        before
    );
    Ok(())
}

async fn prepare_metadata_claim_reservation(
    scope: &crate::journal::connection::ConnectionJournal,
    nonce: char,
) -> Result<
    (
        i64,
        crate::journal::StorageProvisioningPlan,
        String,
        String,
        String,
    ),
    HostError,
> {
    use crate::journal::{
        AssignedAcquireResult, AssignmentKey, RequestPhysicalReservationResult, ReservationRequest,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    let nonce = nonce.to_string().repeat(32);
    let sample_generation = format!("{:032x}", nonce.as_bytes()[0]);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.clone(),
        })
        .await?;
    let permit =
        StartPermit::test_fixture_with_engine_id(&sample_generation, helpers::TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let request = helpers::acquired_physical_fixture_request(scope).await?;
    let result = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &assignment,
                engine_id: helpers::TEST_ENGINE_ID,
                docker_root_digest: &root_digest,
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
        intent_id,
    } = result
    else {
        return Err(HostError::Journal);
    };
    scope
        .record_request_acquire(
            &request,
            AssignedAcquireResult::Returned(vec![request.request_id().ok_or(HostError::Identity)?]),
        )
        .await?;
    if scope.begin_launch(&assignment).await? != (intent_id, false) {
        return Err(HostError::Journal);
    }
    let plan = scope
        .storage_provisioning_plan(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    Ok((
        reservation_id,
        plan,
        helpers::TEST_ENGINE_ID.to_owned(),
        root_digest,
        nonce,
    ))
}

async fn initializer_fixture_state(
    scope: &crate::journal::connection::ConnectionJournal,
    reservation_id: i64,
    metadata_name: &str,
) -> Result<(i64, i64, i64, String, String), HostError> {
    read_transaction(scope.journal_ref(), async |connection| {
        let mut initializer = connection
            .query(
                "SELECT COUNT(*) FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let initializer_count = initializer
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?;
        let mut reservation = connection
            .query(
                "SELECT state, cpu_millicores, memory_bytes FROM capacity_reservations WHERE id = ?1",
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let reservation = reservation.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let state = reservation.get::<String>(0).map_err(|_| HostError::Journal)?;
        let cpu = reservation.get::<i64>(1).map_err(|_| HostError::Journal)?;
        let memory = reservation.get::<i64>(2).map_err(|_| HostError::Journal)?;
        let mut metadata = connection
            .query(
                "SELECT create_state, COALESCE(docker_id, '') FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1 AND role = 'engine-metadata' AND docker_name = ?2",
                (reservation_id, metadata_name),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let metadata = metadata.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        Ok((
            initializer_count,
            cpu,
            memory,
            state,
            format!("{}:{}", metadata.get::<String>(0).map_err(|_| HostError::Journal)?, metadata.get::<String>(1).map_err(|_| HostError::Journal)?),
        ))
    })
    .await
}
