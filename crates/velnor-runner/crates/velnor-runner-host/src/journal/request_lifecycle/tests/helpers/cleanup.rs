pub(super) async fn record_owned_storage_cleanup(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
) -> Result<(), HostError> {
    record_cleanup_observer_probe(scope, provisioned).await?;
    record_owned_storage_cleanup_proof(scope, provisioned).await
}

pub(super) async fn record_owned_storage_cleanup_proof(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
) -> Result<(), HostError> {
    use crate::journal::StorageRole;
    use crate::launch::bootstrap::test_verified_storage_cleanup;
    use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

    let names = WorkerIdentityNames::from_nonce(&provisioned.nonce)?;
    let proof = test_verified_storage_cleanup(
        provisioned.reservation_id,
        provisioned.intent_id,
        TEST_ENGINE_ID.to_owned(),
        provisioned.docker_root_digest.clone(),
        provisioned.runner_container_id.clone(),
        provisioned.dind_container_id.clone(),
        vec![
            names.volume_name(WorkerVolumeRole::Socket),
            names.volume_name(WorkerVolumeRole::Home),
            names.volume_name(WorkerVolumeRole::Work),
            names.volume_name(WorkerVolumeRole::DindData),
            names.volume_name(WorkerVolumeRole::RunnerMetadata),
        ],
        provisioned.instance_binding_digest.clone(),
        StorageRole::PHYSICAL.to_vec(),
        true,
        true,
        true,
        true,
    )?;
    scope.record_storage_cleanup(proof).await
}

async fn record_cleanup_observer_probe(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
) -> Result<(), HostError> {
    use std::fmt::Write as _;

    use crate::journal::{ProbeSeed, ResourceProbePurpose};
    use sha2::{Digest, Sha256};
    use velnor_resource_probe::cleanup_observer_wire::CleanupObserverRequest;

    let journal = scope.journal_ref();
    configure_cleanup_probe_capacity(journal).await?;
    let witness = scope
        .storage_cleanup_witness(provisioned.reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let operation_id = provisioned.nonce.clone();
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    let request = CleanupObserverRequest {
        schema_version: 1,
        operation_id: operation_id.clone(),
        binding_sha256: witness.binding_sha256().to_owned(),
        binding: witness.binding().clone(),
    };
    let request_json = serde_json::to_vec(&request).map_err(|_| HostError::Journal)?;
    let mut digest = Sha256::new();
    digest.update(b"velnor-cleanup-observer-request-v1\0");
    digest.update(&request_json);
    digest.update(b"\n");
    let mut request_sha256 = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut request_sha256, "{byte:02x}")
            .expect("writing to a String cannot fail");
    }
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: journal.instance_id().await?,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: provisioned.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: operation_name.clone(),
        projection_digest: "e".repeat(64),
        purpose: ResourceProbePurpose::CleanupObserver,
        request_sha256,
    };
    journal
        .prepare_cleanup_observer_probe(scope, provisioned.reservation_id, seed)
        .await?;
    retire_cleanup_observer_probe(journal, provisioned, operation_id, operation_name).await
}

async fn retire_cleanup_observer_probe(
    journal: &crate::journal::Journal,
    provisioned: &ProvisionedRunner,
    operation_id: String,
    operation_name: String,
) -> Result<(), HostError> {
    use crate::journal::ProbePhase;
    use crate::launch::VerifiedProbeCleanup;

    journal
        .transition_probe(
            &operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let helper_id = "a".repeat(64);
    journal.bind_probe_container(&operation_id, &helper_id).await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await?;
    journal
        .retire_probe(VerifiedProbeCleanup::test_only(
            operation_id,
            TEST_ENGINE_ID.to_owned(),
            provisioned.docker_root_digest.clone(),
            ProbePhase::RemoveRequested,
            operation_name,
            Some(helper_id),
        ))
        .await
}

async fn configure_cleanup_probe_capacity(
    journal: &crate::journal::Journal,
) -> Result<(), HostError> {
    use crate::journal::{CapacityLimits, OperationalCapacityLimits};
    use crate::journal::connection::storage::read_transaction;

    let limits = read_transaction(journal, async |connection| {
        let mut rows = connection
            .query(
                "SELECT max_jobs, max_cpu_millicores, max_memory_bytes FROM host_capacity_policy WHERE singleton = 1",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        Ok(CapacityLimits {
            jobs: u32::try_from(row.get::<i64>(0).map_err(|_| HostError::Journal)?)
                .map_err(|_| HostError::Journal)?,
            cpu_millicores: u64::try_from(row.get::<i64>(1).map_err(|_| HostError::Journal)?)
                .map_err(|_| HostError::Journal)?,
            memory_bytes: u64::try_from(row.get::<i64>(2).map_err(|_| HostError::Journal)?)
                .map_err(|_| HostError::Journal)?,
        })
    })
    .await?;
    journal
        .configure_capacity_with_operational_limits(
            limits,
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await
}

pub(super) async fn record_observed_storage_cleanup(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(), HostError> {
    let proof = crate::launch::bootstrap::verify_storage_cleanup_for_scope(
        scope,
        scope.journal_ref(),
        reservation_id,
    )
    .await?;
    scope.record_storage_cleanup(proof).await
}

pub(super) async fn record_owned_network_cleanup(
    scope: &ConnectionJournal,
    reservation_id: i64,
) -> Result<(), HostError> {
    use crate::launch::bootstrap::VerifiedStorageNetworkRemoved;

    let claim = scope
        .claim_storage_network_removal(reservation_id)
        .await?
        .ok_or(HostError::Journal)?;
    let proof = VerifiedStorageNetworkRemoved::test_removed(&claim);
    scope.record_storage_network_removed(&claim, &proof).await
}

pub(super) async fn settle_storage_metadata(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    probe_nonce: char,
) -> Result<crate::journal::MetadataSettlementResult, HostError> {
    let claim = create_cleaned_settlement_probe(scope, provisioned, probe_nonce).await?;
    let sample = settlement_sample(scope, &claim).await?;
    scope
        .record_storage_metadata_settlement(&claim, sample)
        .await
}

pub(super) async fn record_runner_completion_for_cleanup(
    scope: &ConnectionJournal,
    request: &crate::journal::AssignmentIdentity,
    intent_id: i64,
    started_job_id: Option<i64>,
    completed_job_id: i64,
    source: &velnor_runner_github::SourceMetadata,
    runner: &velnor_runner_github::RunnerReference,
) -> Result<i64, HostError> {
    if let Some(job_id) = started_job_id
        && scope
            .record_job_started(request, job_id, source, runner, None)
            .await?
            != intent_id
    {
        return Err(HostError::Journal);
    }
    if scope
        .record_job_completed(
            request,
            completed_job_id,
            source,
            Some("success"),
            Some(runner),
            None,
        )
        .await?
        != Some(intent_id)
    {
        return Err(HostError::Journal);
    }
    let claim = scope
        .claim_completion_cleanup(intent_id, 180)
        .await?
        .ok_or(HostError::Journal)?;
    Ok(claim.generation)
}

pub(super) async fn record_runner_absent_for_cleanup(
    scope: &ConnectionJournal,
    intent_id: i64,
    generation: i64,
) -> Result<(), HostError> {
    if !scope
        .record_completion_runner_absent(intent_id, generation)
        .await?
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) async fn create_cleaned_settlement_probe(
    scope: &ConnectionJournal,
    provisioned: &ProvisionedRunner,
    probe_nonce: char,
) -> Result<crate::journal::StorageMetadataSettlementClaim, HostError> {
    use crate::journal::{OperationalCapacityLimits, ProbePhase, ProbeSeed};
    use crate::launch::VerifiedProbeCleanup;

    let journal = scope.journal_ref();
    trace_test_stage!(
        "settlement capacity",
        journal.configure_capacity_with_operational_limits(
            super::admission::JOB_LIMIT,
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
    );
    let operation_id = probe_nonce.to_string().repeat(32);
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: trace_test_stage!("settlement instance", journal.instance_id()),
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: provisioned.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: operation_name.clone(),
        projection_digest: "e".repeat(64),
        purpose: crate::journal::ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    };
    let claim = trace_test_stage!(
        "settlement probe claim",
        scope.prepare_storage_metadata_settlement_probe(provisioned.reservation_id, seed)
    );
    trace_test_stage!(
        "settlement create claim",
        journal.transition_probe(
            &operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
    );
    let container_id = "f".repeat(64);
    trace_test_stage!(
        "settlement bind probe",
        journal.bind_probe_container(&operation_id, &container_id)
    );
    trace_test_stage!(
        "settlement remove claim",
        journal.transition_probe(
            &operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
    );
    trace_test_stage!(
        "settlement probe retire",
        journal.retire_probe(VerifiedProbeCleanup::test_only(
            operation_id,
            TEST_ENGINE_ID.to_owned(),
            provisioned.docker_root_digest.clone(),
            ProbePhase::RemoveRequested,
            operation_name,
            Some(container_id),
        ))
    );
    Ok(claim)
}

pub(super) async fn settlement_sample(
    scope: &ConnectionJournal,
    claim: &crate::journal::StorageMetadataSettlementClaim,
) -> Result<crate::journal::StorageAdmissionSample, HostError> {
    use crate::journal::{
        StorageAdmissionSample, StorageFilesystemKind, StorageMedium, StoragePoolObservation,
        StoragePoolObservationInput,
        StorageSampleIdentity,
    };

    let profile = trace_test_stage!("settlement profile", scope.current_storage_profile())
        .ok_or(HostError::Journal)?;
    let disk_pool = profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .ok_or(HostError::Journal)?;
    let identity = StorageSampleIdentity::new(
        claim.probe_operation_id().to_owned(),
        claim.profile_generation_id().to_owned(),
        claim.profile_digest().to_owned(),
        claim.engine_id().to_owned(),
        claim.docker_root_digest().to_owned(),
    )?;
    let pool = StoragePoolObservation::new_with_filesystem(StoragePoolObservationInput {
        pool_id: disk_pool.pool_id().to_owned(),
        filesystem_instance_digest: "a".repeat(64),
        medium: StorageMedium::Disk,
        filesystem_kind: StorageFilesystemKind::Btrfs,
        total_bytes: 64_u64 << 30,
        available_bytes: 60_u64 << 30,
        total_inodes: 0,
        available_inodes: 0,
    })?;
    StorageAdmissionSample::new(identity, Instant::now(), vec![pool], None)
}
