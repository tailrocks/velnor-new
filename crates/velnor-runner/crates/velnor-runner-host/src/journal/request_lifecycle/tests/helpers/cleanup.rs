pub(super) async fn record_owned_storage_cleanup(
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
        request_sha256: "5d76d86293261804706a16a73e0491cf4caa859f1b876cda706e568592b85cdf".to_owned(),
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
    let pool = StoragePoolObservation::new_with_filesystem(
        disk_pool.pool_id().to_owned(),
        "a".repeat(64),
        StorageMedium::Disk,
        StorageFilesystemKind::Btrfs,
        64_u64 << 30,
        60_u64 << 30,
        0,
        0,
    )?;
    StorageAdmissionSample::new(identity, Instant::now(), vec![pool], None)
}
