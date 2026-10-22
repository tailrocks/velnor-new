use std::time::{Duration, Instant};

use bollard::Docker;

use crate::{
    error::HostError,
    journal::{
        ConnectionJournal, Journal, MetadataSettlementResult, StorageAdmissionSample,
        StorageFilesystemKind, StorageInodePolicy, StorageMedium, StoragePoolLimit,
        StoragePoolObservation, StorageProfile, StorageRole, StorageSampleIdentity,
    },
};

use super::super::lifecycle::{guest_capacity, info, same_guest};
use super::super::record::InodePolicy;
use super::claim::PendingSettlement;

pub(super) async fn settle_sample(
    docker: &Docker,
    journal: &Journal,
    scope: &ConnectionJournal,
    pending: PendingSettlement,
) -> Result<MetadataSettlementResult, HostError> {
    let PendingSettlement {
        profile,
        evidence,
        engine_id,
        root,
        architecture,
        cpu_count,
        memory_total,
        deadline,
        projection,
        claim,
    } = pending;
    let output = Box::pin(super::super::execute::run(
        docker,
        journal,
        &projection,
        evidence,
        memory_total,
        &deadline,
    ))
    .await?;
    verify_output_evidence(&output, &projection.operation_id, &engine_id, root.digest())?;
    let after = info(docker, &deadline).await?;
    if !same_guest(&after, &engine_id, &root, &architecture)?
        || guest_capacity(&after)? != (cpu_count, memory_total)
    {
        return Err(HostError::Identity);
    }
    let sample = settlement_sample(&output, &profile, &claim)?;
    scope
        .record_storage_metadata_settlement(&claim, sample)
        .await
}

fn verify_output_evidence(
    output: &super::super::execute::ObservedProbeRecord,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<(), HostError> {
    let startup = output
        .startup_evidence
        .as_ref()
        .ok_or(HostError::Identity)?;
    verify_startup_evidence(Some(startup), operation_id, engine_id, docker_root_digest)
}

pub(super) fn verify_startup_evidence(
    startup: Option<&super::super::execute::ProbeStartupEvidence>,
    operation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<(), HostError> {
    let startup = startup.ok_or(HostError::Identity)?;
    let cleanup = startup.cleanup();
    let image = startup.image();
    if cleanup.operation_id() != operation_id
        || cleanup.engine_id() != engine_id
        || cleanup.docker_root_digest() != docker_root_digest
        || image.engine_id() != engine_id
        || image.docker_root_digest() != docker_root_digest
        || image.platform() != "linux/amd64"
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn settlement_sample(
    output: &super::super::execute::ObservedProbeRecord,
    profile: &StorageProfile,
    claim: &crate::journal::StorageMetadataSettlementClaim,
) -> Result<StorageAdmissionSample, HostError> {
    if Instant::now()
        .checked_duration_since(output.observed_at)
        .is_none_or(|age| age > Duration::from_secs(30))
        || output.sample_generation_id != claim.probe_operation_id()
        || claim.profile_generation_id() != profile.generation_id()
        || claim.profile_digest() != profile.profile_digest()
    {
        return Err(HostError::Identity);
    }
    settlement_sample_for_identity(
        output,
        profile,
        claim.probe_operation_id(),
        claim.profile_generation_id(),
        claim.profile_digest(),
        claim.engine_id(),
        claim.docker_root_digest(),
    )
}

pub(super) fn settlement_sample_for_identity(
    output: &super::super::execute::ObservedProbeRecord,
    profile: &StorageProfile,
    operation_id: &str,
    profile_generation_id: &str,
    profile_digest: &str,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<StorageAdmissionSample, HostError> {
    if Instant::now()
        .checked_duration_since(output.observed_at)
        .is_none_or(|age| age > Duration::from_secs(30))
        || output.sample_generation_id != operation_id
        || profile_generation_id != profile.generation_id()
        || profile_digest != profile.profile_digest()
    {
        return Err(HostError::Identity);
    }
    let pool = metadata_pool(profile)?;
    let record = &output.record;
    let filesystem_kind = match record.docker_root_fs_type_magic {
        0x9123_683e => StorageFilesystemKind::Btrfs,
        0x0102_1994 => StorageFilesystemKind::Tmpfs,
        _ => StorageFilesystemKind::Other,
    };
    let inode_policy = match record.docker_root_inode_policy {
        InodePolicy::Finite => StorageInodePolicy::Finite,
        InodePolicy::DynamicBtrfs => StorageInodePolicy::DynamicBtrfs,
        InodePolicy::Unknown => StorageInodePolicy::Unknown,
    };
    if pool.inode_policy() != inode_policy {
        return Err(HostError::Identity);
    }
    let identity = StorageSampleIdentity::new(
        operation_id.to_owned(),
        profile_generation_id.to_owned(),
        profile_digest.to_owned(),
        engine_id.to_owned(),
        docker_root_digest.to_owned(),
    )?;
    let filesystem_instance_digest =
        super::super::filesystem_instance_digest(super::super::FilesystemInstanceDigestInput {
            engine_id,
            docker_root_digest,
            guest_boot_id: record.guest_boot_id.as_bytes(),
            mount_source: record.docker_root_mount_source.as_bytes(),
            filesystem_type_name: record.docker_root_fs_type_name.as_bytes(),
            filesystem_type_magic: record.docker_root_fs_type_magic,
            mount_device_major: record.docker_root_device_major,
            mount_device_minor: record.docker_root_device_minor,
            statx_device_major: record.docker_root_statx_device_major,
            statx_device_minor: record.docker_root_statx_device_minor,
            filesystem_id: record.docker_root_filesystem_id,
        });
    let pool_observation = StoragePoolObservation::new_with_filesystem(
        pool.pool_id().to_owned(),
        filesystem_instance_digest,
        StorageMedium::Disk,
        filesystem_kind,
        record.docker_root_total_bytes,
        record.docker_root_available_bytes,
        record.docker_root_inode_total,
        record.docker_root_inode_available,
    )?;
    let sample =
        StorageAdmissionSample::new(identity, output.observed_at, vec![pool_observation], None)?;
    sample.validate_metadata_settlement_for(
        profile,
        operation_id,
        engine_id,
        docker_root_digest,
        Instant::now(),
    )?;
    Ok(sample)
}

fn metadata_pool(profile: &StorageProfile) -> Result<&StoragePoolLimit, HostError> {
    profile
        .validate_provisioning_contract()
        .map_err(|_| HostError::Identity)?;
    let disk_pools = profile
        .pools()
        .iter()
        .filter(|pool| pool.medium() == StorageMedium::Disk)
        .collect::<Vec<_>>();
    if disk_pools.len() != 1 {
        return Err(HostError::Identity);
    }
    let role = profile
        .role_limit(StorageRole::EngineMetadata)
        .map_err(|_| HostError::Identity)?;
    let pool = profile
        .pool_limit(role.pool_id())
        .map_err(|_| HostError::Identity)?;
    if disk_pools[0].pool_id() != pool.pool_id() || pool.medium() != StorageMedium::Disk {
        return Err(HostError::Identity);
    }
    Ok(pool)
}
