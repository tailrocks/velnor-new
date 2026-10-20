pub(crate) async fn provision_physical_intent_with_initialization(
    scope: &ConnectionJournal,
    nonce: char,
    sample_nonce: char,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::AssignmentKey;
    let nonce_text = nonce.to_string().repeat(32);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce_text.clone(),
        })
        .await?;
    provision_assignment_with_initialization(scope, assignment, None, nonce, sample_nonce, None).await
}

pub(crate) async fn provision_acquired_request_with_initialization(
    scope: &ConnectionJournal,
    request_id: i64,
    nonce: char,
    sample_nonce: char,
) -> Result<JobFactsReaderFixture, HostError> {
    provision_acquired_request_with_initialization_for_repository(
        scope,
        request_id,
        nonce,
        sample_nonce,
        101,
    )
    .await
}

pub(crate) async fn provision_acquired_request_with_initialization_for_repository(
    scope: &ConnectionJournal,
    request_id: i64,
    nonce: char,
    sample_nonce: char,
    repository_id: i64,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::{AssignmentKey, SourceAuthorizationResult};
    let request = scope
        .assignment_identity(crate::journal::AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source = receipt(scope, request_id, request_id + 100, repository_id).await?;
    if scope.record_source_authorization(&request, &source).await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    provision_assignment_with_initialization(
        scope,
        assignment,
        Some(request),
        nonce,
        sample_nonce,
        None,
    )
    .await
}

pub(crate) async fn provision_acquired_request_with_initialization_and_filesystem_digest(
    scope: &ConnectionJournal,
    request_id: i64,
    nonce: char,
    sample_nonce: char,
    filesystem_instance_digest: String,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::AssignmentKey;

    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source = receipt(scope, request_id, request_id + 100, 101).await?;
    if scope.record_source_authorization(&request, &source).await?
        != crate::journal::SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    provision_assignment_with_initialization(
        scope,
        assignment,
        Some(request),
        nonce,
        sample_nonce,
        Some(filesystem_instance_digest),
    )
    .await
}

async fn provision_assignment_with_initialization(
    scope: &ConnectionJournal,
    assignment: crate::journal::AssignmentIdentity,
    request: Option<crate::journal::AssignmentIdentity>,
    nonce: char,
    sample_nonce: char,
    filesystem_instance_digest: Option<String>,
) -> Result<JobFactsReaderFixture, HostError> {
    use crate::journal::{
        ReservationRequest, ReservationResult, fixture_storage_provisioning_admission,
        fixture_storage_provisioning_admission_with_filesystem_digest,
    };
    use crate::launch::StartPermit;

    const JOB_MEMORY: u64 = 8_589_934_592;
    let sample_generation = sample_generation_for_test(sample_nonce);
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let admission = match filesystem_instance_digest.as_ref() {
        Some(digest) => fixture_storage_provisioning_admission_with_filesystem_digest(
            &permit,
            digest.clone(),
        )?,
        None => fixture_storage_provisioning_admission(&permit)?,
    };
    let physical = ReservationRequest {
        assignment: &assignment,
        engine_id: TEST_ENGINE_ID,
        docker_root_digest: &docker_root_digest,
        cpu_millicores: 2_000,
        memory_bytes: JOB_MEMORY,
        sample_generation_id: &sample_generation_id,
    };
    let reservation_id = if let Some(request) = request.as_ref() {
        let crate::journal::RequestPhysicalReservationResult::AcquireClaimed {
            reservation_id, ..
        } = scope
            .try_reserve_and_claim_request_acquire(physical, permit, admission, request)
            .await?
        else {
            return Err(HostError::Journal);
        };
        scope
            .record_request_acquire(
                request,
                crate::journal::AssignedAcquireResult::Returned(vec![
                    request.request_id().ok_or(HostError::Identity)?,
                ]),
            )
            .await?;
        reservation_id
    } else {
        let ReservationResult::Reserved { reservation_id } = scope
            .try_reserve_with_provisioning(physical, permit, admission)
            .await?
        else {
            return Err(HostError::Journal);
        };
        reservation_id
    };
    let (intent_id, fresh) = scope.begin_launch(&assignment).await?;
    if fresh {
        return Err(HostError::Journal);
    }
    let artifacts = match filesystem_instance_digest {
        Some(digest) => Box::pin(
            provisioning::provision_with_initialization_and_filesystem_digest(
                scope,
                assignment,
                nonce,
                reservation_id,
                intent_id,
                docker_root_digest,
                sample_generation_id,
                digest,
            ),
        )
        .await?,
        None => Box::pin(provisioning::provision_with_initialization(
            scope,
            assignment,
            nonce,
            reservation_id,
            intent_id,
            docker_root_digest,
            sample_generation_id,
        ))
        .await?,
    };
    let provisioned = artifacts.provisioned;
    Ok(JobFactsReaderFixture {
        // Physical reservation stays bound to the Remote assignment, while
        // acquired request events use the exact AssignedRequest identity.
        assignment: request.unwrap_or_else(|| provisioned.assignment.clone()),
        reservation_assignment: provisioned.assignment.clone(),
        provisioned: provisioned.clone(),
        reservation_id: provisioned.reservation_id,
        intent_id: provisioned.intent_id,
        runner_container_id: provisioned.runner_container_id,
        dind_container_id: provisioned.dind_container_id,
        docker_root_digest: provisioned.docker_root_digest,
        sample_generation_id: provisioned.sample_generation_id,
        instance_binding_digest: provisioned.instance_binding_digest,
        claim: artifacts.claim,
        handoff: artifacts.handoff,
        observed: artifacts.observed,
        ready: artifacts.ready,
    })
}

pub(crate) struct JobFactsReaderFixture {
    pub(crate) assignment: crate::journal::AssignmentIdentity,
    pub(crate) reservation_assignment: crate::journal::AssignmentIdentity,
    pub(super) provisioned: ProvisionedRunner,
    pub(crate) reservation_id: i64,
    pub(crate) intent_id: i64,
    pub(crate) runner_container_id: String,
    pub(crate) dind_container_id: String,
    pub(crate) docker_root_digest: String,
    pub(crate) sample_generation_id: String,
    pub(crate) instance_binding_digest: String,
    pub(crate) claim: crate::journal::StorageInitializationClaim,
    pub(crate) handoff: crate::journal::VerifiedMetadataInitializationHandoff,
    pub(crate) observed: crate::worker::storage_initialization::ObservedStorageInitialization,
    pub(crate) ready: crate::worker::storage_initialization::VerifiedStorageInitializationReady,
}

pub(crate) async fn initialize_reserved_request(
    scope: &ConnectionJournal,
    request: crate::journal::AssignmentIdentity,
    physical: crate::journal::AssignmentIdentity,
    nonce: char,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<JobFactsReaderFixture, HostError> {
    let artifacts = Box::pin(provisioning::provision_with_initialization(
        scope,
        physical.clone(),
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    ))
    .await?;
    let provisioned = artifacts.provisioned;
    Ok(JobFactsReaderFixture {
        assignment: request,
        reservation_assignment: physical,
        reservation_id: provisioned.reservation_id,
        intent_id: provisioned.intent_id,
        runner_container_id: provisioned.runner_container_id.clone(),
        dind_container_id: provisioned.dind_container_id.clone(),
        docker_root_digest: provisioned.docker_root_digest.clone(),
        sample_generation_id: provisioned.sample_generation_id.clone(),
        instance_binding_digest: provisioned.instance_binding_digest.clone(),
        provisioned,
        claim: artifacts.claim,
        handoff: artifacts.handoff,
        observed: artifacts.observed,
        ready: artifacts.ready,
    })
}

pub(crate) async fn provision_acquired_physical_intent(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: crate::journal::AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<(), HostError> {
    provisioning::provision_with_nonce(
        scope,
        engine_id,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
    .map(|_| ())
}

pub(crate) async fn prepare_bootstrap_start_claim(
    scope: &ConnectionJournal,
    engine_id: &str,
    assignment: crate::journal::AssignmentIdentity,
    nonce: &str,
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    sample_generation_id: String,
) -> Result<
    (
        crate::journal::BootstrapStartClaim,
        crate::journal::VerifiedMetadataInitializationHandoff,
    ),
    HostError,
> {
    provisioning::prepare_bootstrap_start_claim(
        scope,
        engine_id,
        assignment,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}
