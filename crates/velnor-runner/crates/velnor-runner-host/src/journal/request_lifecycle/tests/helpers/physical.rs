#[derive(Clone)]
pub(crate) struct ProvisionedRunner {
    pub(super) assignment: crate::journal::AssignmentIdentity,
    pub(super) intent_id: i64,
    pub(super) reservation_id: i64,
    pub(super) runner_container_id: String,
    pub(super) dind_container_id: String,
    pub(super) docker_root_digest: String,
    pub(super) sample_generation_id: String,
    pub(super) instance_binding_digest: String,
    pub(super) storage_plan: crate::journal::StorageProvisioningPlan,
    pub(super) nonce: String,
}

pub(crate) struct BootstrapStartClaimFixture {
    pub(crate) claim: crate::journal::BootstrapStartClaim,
    pub(crate) metadata_handoff: crate::journal::VerifiedMetadataInitializationHandoff,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) reservation_id: i64,
}

pub(crate) async fn prepare_bootstrap_start_claim_for_nonce(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<BootstrapStartClaimFixture, HostError> {
    use crate::journal::{
        AssignedAcquireResult, AssignmentKey, RequestPhysicalReservationResult, ReservationRequest,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let nonce_text = nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text.clone(),
        })
        .await?;
    let sample_generation = sample_generation_for_test(nonce);
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let request = acquired_physical_fixture_request(scope).await?;
    let reservation = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
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
    let (claim, metadata_handoff) =
        provisioning::prepare_bootstrap_start_claim(provisioning::ProvisioningFixture {
            scope,
            engine_id: TEST_ENGINE_ID.to_owned(),
            assignment,
            nonce: nonce_text,
            reservation_id,
            intent_id,
            docker_root_digest: docker_root_digest.clone(),
            sample_generation_id,
            filesystem_instance_digest: "a".repeat(64),
        })
        .await?;
    if claim.operation_id() != intent_id {
        return Err(HostError::Journal);
    }
    Ok(BootstrapStartClaimFixture {
        claim,
        metadata_handoff,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest,
        reservation_id,
    })
}

pub(super) async fn provision_physical_intent(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<ProvisionedRunner, HostError> {
    provision_physical_intent_with_sample(scope, nonce, nonce).await
}

pub(super) async fn provision_physical_intent_with_sample(
    scope: &ConnectionJournal,
    nonce: char,
    sample_nonce: char,
) -> Result<ProvisionedRunner, HostError> {
    use crate::journal::{
        AssignedAcquireResult, AssignmentKey, RequestPhysicalReservationResult, ReservationRequest,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let nonce_text = nonce.to_string().repeat(32);
    let sample_generation = sample_generation_for_test(sample_nonce);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text,
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let request = acquired_physical_fixture_request(scope).await?;
    let result = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
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
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh || intent_id != acquired_intent_id {
        return Err(HostError::Journal);
    }

    provision_reserved_physical_intent(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}

pub(crate) async fn prepare_physical_initializer(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<i64, HostError> {
    use crate::journal::{
        AssignedAcquireResult, AssignmentKey, RequestPhysicalReservationResult, ReservationRequest,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let nonce_text = nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text,
        })
        .await?;
    let sample = sample_generation_for_test(nonce);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let request = acquired_physical_fixture_request(scope).await?;
    let result = scope
        .try_reserve_and_claim_request_acquire(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
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
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh || intent_id != acquired_intent_id {
        return Err(HostError::Journal);
    }
    provisioning::prepare_initializer_state(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await?;
    Ok(reservation_id)
}

pub(crate) async fn prepare_legacy_physical_initializer(
    scope: &ConnectionJournal,
    nonce: char,
) -> Result<i64, HostError> {
    use crate::journal::{
        AssignmentKey, ReservationRequest, ReservationResult,
        fixture_storage_provisioning_admission,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let nonce_text = nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text,
        })
        .await?;
    let sample = sample_generation_for_test(nonce);
    let permit = StartPermit::test_fixture_with_engine_id(&sample, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let result = scope
        .try_reserve_with_provisioning(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &docker_root_digest,
                cpu_millicores: 2_000,
                memory_bytes: JOB_MEMORY,
                sample_generation_id: &sample_generation_id,
            },
            permit,
            admission,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh {
        return Err(HostError::Journal);
    }
    provisioning::prepare_initializer_state(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await?;
    Ok(reservation_id)
}

fn sample_generation_for_test(nonce: char) -> String {
    format!("{:032x}", nonce as u32)
}

pub(crate) async fn prepare_physical_initializer_with_known_helper(
    scope: &ConnectionJournal,
    nonce: char,
    helper_id: &str,
) -> Result<i64, HostError> {
    use crate::worker::storage_initialization::{
        StorageInitializationImage, StorageInitializerCreateResponse,
    };

    let reservation_id = prepare_physical_initializer(scope, nonce).await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image = StorageInitializationImage::from_verified_release(&images)?;
    let claim = scope
        .claim_storage_initialization(
            reservation_id,
            &image,
            crate::journal::request_lifecycle::tests::helpers::test_initializer_limits()?,
        )
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_storage_initializer_create_dispatched(&claim)
        .await?;
    scope
        .record_storage_initializer_create_response_received(&claim)
        .await?;
    let response = StorageInitializerCreateResponse::test_response(&claim, helper_id);
    scope
        .record_storage_initializer_create_response(&claim, &response)
        .await?;
    scope
        .mark_storage_initializer_create_uncertain(&claim)
        .await?;
    Ok(reservation_id)
}

pub(super) async fn provision_reserved_physical_intent(
    scope: &ConnectionJournal,
    assignment: crate::journal::AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<ProvisionedRunner, HostError> {
    provisioning::provision(
        scope,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}

#[path = "physical/provisioning.rs"]
pub(crate) mod provisioning;

fn role_filesystem_digest(role: crate::journal::StorageRole) -> String {
    use crate::journal::StorageRole;
    match role {
        StorageRole::RunnerRoot | StorageRole::DindRoot | StorageRole::EngineMetadata => {
            "a".repeat(64)
        }
        StorageRole::RunnerHome => "2".repeat(64),
        StorageRole::RunnerWork => "3".repeat(64),
        StorageRole::RunnerTmp => "4".repeat(64),
        StorageRole::RunnerShm => "5".repeat(64),
        StorageRole::SharedRun => "6".repeat(64),
        StorageRole::DindData => "7".repeat(64),
        StorageRole::DindTmp => "8".repeat(64),
        StorageRole::DindShm => "9".repeat(64),
        StorageRole::CacheLive | StorageRole::CacheStage => "a".repeat(64),
    }
}
