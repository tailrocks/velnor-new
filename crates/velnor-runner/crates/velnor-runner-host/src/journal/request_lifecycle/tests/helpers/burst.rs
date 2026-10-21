use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::connection::ConnectionJournal;
use crate::journal::request_lifecycle::tests::{NativeStartedIntakeFixture, helpers};
use crate::journal::{
    AssignedAcquireResult, CapacityLimits, CreatedScaleSetResponse, Journal,
    OperationalCapacityLimits, RequestPhysicalReservationResult, ReservationRequest,
    ScaleSetCreateRequest, SourceAuthorizationResult, StorageAdmissionSample,
    StorageFilesystemKind, StorageMedium, StoragePoolLimit, StoragePoolObservation, StorageProfile,
    StorageProvisioningAdmission, StorageProvisioningMemoryObservation, StorageSampleIdentity,
    fixture_provisioning_profile,
};
use crate::launch::StartPermit;
use velnor_runner_github::RunnerReference;

const BURST_CAPACITY: u64 = 13;
const JOB_MEMORY: u64 = 8_589_934_592;
const WORKER_CPU_MILLICORES: u64 = BURST_CAPACITY * 2_000;
const OPERATIONAL_CPU_MILLICORES: u64 = 1_000;
const GUEST_CPU_MILLICORES: u64 = WORKER_CPU_MILLICORES + OPERATIONAL_CPU_MILLICORES;
const WORKER_MEMORY_BYTES: u64 = 512 * 1024 * 1024 * 1024;
const OPERATIONAL_MEMORY_BYTES: u64 = 128 * 1024 * 1024;
const GUEST_MEMORY_TOTAL_BYTES: u64 = WORKER_MEMORY_BYTES + OPERATIONAL_MEMORY_BYTES;
const GUEST_MEMORY_AVAILABLE_BYTES: u64 = 256 * 1024 * 1024 * 1024;
const DISK_CAPACITY: u64 = 512 * 1024 * 1024 * 1024;
const BURST_NONCES: [char; 12] = ['b', 'c', 'd', 'e', 'f', '0', '1', '2', '3', '4', '5', '6'];

pub(crate) async fn native_started_intake_burst_fixture()
-> Result<NativeStartedIntakeFixture, HostError> {
    const REQUEST_ID: i64 = 101;
    const MESSAGE_ID: i64 = 1_501;
    const SET_ID: i64 = 501;
    const REPOSITORY_ID: i64 = 101;
    const RUN_ID: i64 = REQUEST_ID + 1_000;

    let scratch = helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: u32::try_from(BURST_CAPACITY).map_err(|_| HostError::Journal)?,
                cpu_millicores: WORKER_CPU_MILLICORES,
                memory_bytes: WORKER_MEMORY_BYTES,
            },
            OperationalCapacityLimits {
                cpu_millicores: OPERATIONAL_CPU_MILLICORES,
                memory_bytes: OPERATIONAL_MEMORY_BYTES,
            },
        )
        .await?;
    let binding = helpers::binding("native-started-intake", REPOSITORY_ID)?;
    let scope = create_burst_scope(&journal, &binding, SET_ID).await?;
    let initialized = provision_burst_request(&scope, REQUEST_ID, 'a').await?;
    let retained_provisioned = initialized.provisioned.clone();
    let runner = RunnerReference {
        id: 701,
        name: helpers::runner_name('a'),
        runner_scale_set_id: SET_ID,
    };
    helpers::finish_initialized_listener_delivery(
        &scope,
        &binding,
        &initialized,
        &runner,
        REQUEST_ID,
    )
    .await?;
    let channel = crate::worker::execution_authorization::test_channel_for_ready_job_facts(
        crate::worker::execution_authorization::TestProofChannelIdentity {
            reservation_id: initialized.reservation_id,
            operation_id: initialized.intent_id,
            connection_id: scope.connection_id().as_str().to_owned(),
            scale_set_id: SET_ID,
            witness_request_id: REQUEST_ID,
            engine_id: helpers::TEST_ENGINE_ID.to_owned(),
            docker_root_digest: initialized.docker_root_digest.clone(),
            runner_id: runner.id,
            runner_name: runner.name.clone(),
            runner_container_id: initialized.runner_container_id.clone(),
            dind_container_id: initialized.dind_container_id.clone(),
            instance_binding_digest: initialized.instance_binding_digest.clone(),
        },
        &initialized.ready,
    )?;
    let source = helpers::event_source(REPOSITORY_ID, RUN_ID);
    let database_path = scratch.file();
    Ok(NativeStartedIntakeFixture {
        _scratch: scratch,
        journal,
        database_path,
        scope,
        request_id: REQUEST_ID,
        message_id: MESSAGE_ID,
        intent_id: initialized.intent_id,
        reservation_id: initialized.reservation_id,
        provisioned: retained_provisioned,
        assignment: initialized.assignment,
        reservation_assignment: initialized.provisioned.assignment.clone(),
        runner,
        source,
        channel,
    })
}

async fn create_burst_scope(
    journal: &Journal,
    binding: &RepositoryConnection,
    set_id: i64,
) -> Result<ConnectionJournal, HostError> {
    journal
        .configure_storage_profile(burst_storage_profile()?)
        .await?;
    journal.bind_engine(helpers::TEST_ENGINE_ID).await?;
    let scope = journal.connection_scope(binding).await?;
    let request = ScaleSetCreateRequest::new(binding, 7)?;
    let prepared = scope.prepare_scale_set_create(&request).await?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await?
        .ok_or(HostError::Journal)?;
    let response = CreatedScaleSetResponse::verified(
        &request,
        set_id,
        "integration",
        binding.scale_set.labels(),
        true,
    )?;
    scope.record_scale_set_created(claim, response).await?;
    scope
        .bind_engine_context(
            helpers::TEST_ENGINE_ID,
            "unix:///var/run/docker.sock",
            "linux/amd64",
        )
        .await?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(binding)?;
    scope.install_job_authority_policy(&config).await?;
    Ok(scope)
}

async fn provision_burst_request(
    scope: &ConnectionJournal,
    request_id: i64,
    nonce: char,
) -> Result<helpers::JobFactsReaderFixture, HostError> {
    use crate::journal::AssignmentKey;

    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id })
        .await?;
    let source_receipt = helpers::receipt(scope, request_id, request_id + 100, 101).await?;
    if scope
        .record_source_authorization(&request, &source_receipt)
        .await?
        != SourceAuthorizationResult::Recorded
    {
        return Err(HostError::Journal);
    }
    let physical = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_string().repeat(32),
        })
        .await?;
    let sample_generation_id = format!("{:032x}", nonce as u32);
    let permit = StartPermit::test_fixture_with_guest_capacity(
        &sample_generation_id,
        helpers::TEST_ENGINE_ID,
        GUEST_CPU_MILLICORES,
        GUEST_MEMORY_TOTAL_BYTES,
        GUEST_MEMORY_AVAILABLE_BYTES,
    );
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let admission = burst_admission(&permit, &profile)?;
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let request_spec = ReservationRequest {
        assignment: &physical,
        engine_id: helpers::TEST_ENGINE_ID,
        docker_root_digest: &docker_root_digest,
        cpu_millicores: 2_000,
        memory_bytes: JOB_MEMORY,
        sample_generation_id: &sample_generation_id,
    };
    let (reservation_id, intent_id) = match scope
        .try_reserve_and_claim_request_acquire(request_spec, permit, admission, &request)
        .await?
    {
        RequestPhysicalReservationResult::AcquireClaimed {
            reservation_id,
            intent_id,
        } => (reservation_id, intent_id),
        RequestPhysicalReservationResult::AtCapacity => return Err(HostError::Identity),
        _ => return Err(HostError::Journal),
    };
    scope
        .record_request_acquire(&request, AssignedAcquireResult::Returned(vec![request_id]))
        .await?;
    helpers::initialize_reserved_request(
        scope,
        request,
        physical,
        nonce,
        reservation_id,
        intent_id,
        docker_root_digest,
        sample_generation_id,
    )
    .await
}

fn burst_admission(
    permit: &StartPermit,
    profile: &StorageProfile,
) -> Result<StorageProvisioningAdmission, HostError> {
    let disk = profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?;
    let sample = StorageAdmissionSample::new(
        StorageSampleIdentity::new(
            permit.sample_generation_id().to_owned(),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            permit.engine_id().to_owned(),
            permit.docker_root_digest().to_owned(),
        )?,
        std::time::Instant::now(),
        vec![StoragePoolObservation::new_with_filesystem(
            disk.pool_id().to_owned(),
            "a".repeat(64),
            StorageMedium::Disk,
            StorageFilesystemKind::Btrfs,
            DISK_CAPACITY,
            DISK_CAPACITY - 12 * 1024 * 1024 * 1024,
            0,
            0,
        )?],
        None,
    )?;
    Ok(StorageProvisioningAdmission::new(
        sample,
        StorageProvisioningMemoryObservation::new(
            GUEST_MEMORY_TOTAL_BYTES,
            GUEST_MEMORY_AVAILABLE_BYTES,
        )?,
    ))
}

fn burst_storage_profile() -> Result<StorageProfile, HostError> {
    let base = fixture_provisioning_profile();
    let pools = base
        .pools()
        .iter()
        .map(|pool| {
            if pool.medium() == StorageMedium::Disk {
                StoragePoolLimit::new_with_inode_policy(
                    pool.pool_id().to_owned(),
                    pool.medium(),
                    pool.inode_policy(),
                    DISK_CAPACITY,
                    pool.max_inodes(),
                    pool.reserve_bytes(),
                    pool.reserve_inodes(),
                )
            } else {
                Ok(pool.clone())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    StorageProfile::new("c".repeat(32), pools, base.roles().to_vec())
}

pub(crate) async fn add_twelve_completed_burst_rows(
    fixture: &NativeStartedIntakeFixture,
) -> Result<(), HostError> {
    fixture
        .journal
        .configure_capacity(CapacityLimits {
            jobs: u32::try_from(BURST_CAPACITY).map_err(|_| HostError::Journal)?,
            cpu_millicores: WORKER_CPU_MILLICORES,
            memory_bytes: WORKER_MEMORY_BYTES,
        })
        .await?;
    let binding = helpers::binding("native-started-intake", 101)?;
    for index in 0..12 {
        add_completed_row(fixture, &binding, index).await?;
    }
    Ok(())
}

async fn add_completed_row(
    fixture: &NativeStartedIntakeFixture,
    binding: &RepositoryConnection,
    index: u8,
) -> Result<(), HostError> {
    let request_id = 20_000 + i64::from(index);
    let nonce = BURST_NONCES[usize::from(index)];
    let initialized = provision_burst_request(&fixture.scope, request_id, nonce).await?;
    let runner = RunnerReference {
        id: 900 + i64::from(index),
        name: helpers::runner_name(nonce),
        runner_scale_set_id: fixture.runner.runner_scale_set_id,
    };
    helpers::finish_initialized_listener_delivery(
        &fixture.scope,
        binding,
        &initialized,
        &runner,
        request_id,
    )
    .await?;
    let source = helpers::event_source(101, request_id + 1_000);
    let job_id = format!("burst-job-{index}");
    let started_message_id = 50_000 + i64::from(index) * 2;
    let completed_message_id = started_message_id + 1;
    let started_intent = fixture
        .scope
        .record_job_started(
            &initialized.assignment,
            started_message_id,
            &source,
            &runner,
            Some(&job_id),
        )
        .await?;
    if started_intent != initialized.intent_id {
        return Err(HostError::Journal);
    }
    let completed_intent = fixture
        .scope
        .record_job_completed(
            &initialized.assignment,
            completed_message_id,
            &source,
            Some("Succeeded"),
            Some(&runner),
            Some(&job_id),
        )
        .await?;
    if completed_intent != Some(initialized.intent_id) {
        return Err(HostError::Journal);
    }
    Ok(())
}

#[tokio::test]
async fn burst_profile_accepts_twelve_additional_real_completions() -> Result<(), HostError> {
    let fixture = native_started_intake_burst_fixture().await?;
    add_twelve_completed_burst_rows(&fixture).await
}
