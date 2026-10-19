use super::capacity::{BTRFS_MAGIC, lowercase_hex};
use super::worker::MeasuredAttachment;
use crate::{
    error::HostError,
    journal::{ConnectionJournal, StorageProvisioningPlan, StorageRole},
    launch::{SelectedEngineContext, StartObservationWitness, VerifiedWorkerImages},
};
use sha2::{Digest, Sha256};
use velnor_resource_probe::{
    InodePolicy,
    worker_binding::{
        AttachmentRole, OwnedCgroupObservation, RootBackingKind, RootBackingObservation, WorkerRole,
    },
};

pub(super) fn validate_owned_sample(
    observation: &OwnedCgroupObservation,
    witness: &StartObservationWitness,
    engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    let sampled = witness.storage_mount();
    let live = &observation.engine_metadata;
    if observation.guest_boot_id != sampled.guest_boot_id()
        || live.target != sampled.mount_target()
        || live.mount_source != sampled.mount_source()
        || live.mount_root != sampled.mount_root()
        || live.mount_id != sampled.mount_id()
        || live.mount_device_major != sampled.device_major()
        || live.mount_device_minor != sampled.device_minor()
        || live.statx_device_major != sampled.statx_device_major()
        || live.statx_device_minor != sampled.statx_device_minor()
        || live.filesystem_type_magic != sampled.filesystem_type_magic()
        || live.filesystem_type_name != sampled.filesystem_type_name()
        || live.filesystem_id != sampled.filesystem_id()
        || observation.engine_metadata.role != AttachmentRole::EngineMetadata
        || !live.read_only
        || engine.engine_id().is_empty()
        || engine.docker_root_digest().is_empty()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn root_backing_mappings(
    plan: &StorageProvisioningPlan,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    attachments: &[MeasuredAttachment],
    observation: &OwnedCgroupObservation,
    witness: &StartObservationWitness,
) -> Result<Vec<super::super::super::VerifiedRootBackingMapping>, HostError> {
    let rows = observation.root_backings.as_slice();
    let [runner, dind] = rows else {
        return Err(HostError::Identity);
    };
    let runner_mapping = root_backing_mapping(
        plan,
        journal,
        engine,
        images,
        attachments,
        runner,
        WorkerRole::Runner,
        StorageRole::RunnerRoot,
        &observation.guest_boot_id,
        witness,
    )?;
    let dind_mapping = root_backing_mapping(
        plan,
        journal,
        engine,
        images,
        attachments,
        dind,
        WorkerRole::Dind,
        StorageRole::DindRoot,
        &observation.guest_boot_id,
        witness,
    )?;
    if runner_mapping.backing_filesystem_digest() != dind_mapping.backing_filesystem_digest() {
        return Err(HostError::Identity);
    }
    Ok(vec![runner_mapping, dind_mapping])
}

fn root_backing_mapping(
    plan: &StorageProvisioningPlan,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    attachments: &[MeasuredAttachment],
    row: &RootBackingObservation,
    worker_role: WorkerRole,
    role: StorageRole,
    boot_id: &str,
    witness: &StartObservationWitness,
) -> Result<super::super::super::VerifiedRootBackingMapping, HostError> {
    let expected_container = match worker_role {
        WorkerRole::Runner => row.container_id.as_str(),
        WorkerRole::Dind => row.container_id.as_str(),
    };
    let logical = attachments
        .iter()
        .find(|item| item.role == role && item.binding.container_id() == Some(expected_container))
        .ok_or(HostError::Identity)?;
    let expected_image = match worker_role {
        WorkerRole::Runner => images.runner(),
        WorkerRole::Dind => images.dind(),
    };
    let directories = row.directories.as_slice();
    let upper = directories
        .iter()
        .find(|directory| directory.kind == RootBackingKind::Upper)
        .ok_or(HostError::Identity)?;
    let work = directories
        .iter()
        .find(|directory| directory.kind == RootBackingKind::Work)
        .ok_or(HostError::Identity)?;
    let lower_count = directories
        .iter()
        .filter(|directory| directory.kind == RootBackingKind::Lower)
        .count();
    let sampled = witness.storage_mount();
    if row.container_id != expected_container
        || row.role != worker_role
        || row.guest_boot_id != boot_id
        || row.mount_id != logical.binding.mount_id()
        || row.mount_source.is_empty()
        || row.filesystem_type != "overlay"
        || row.read_only
        || row.upperdir.is_empty()
        || row.workdir.is_empty()
        || lower_count != row.lowerdirs.len()
        || row.directories.len() != lower_count + 2
        || upper.path != row.upperdir
        || work.path != row.workdir
        || row.lowerdirs.iter().any(|path| {
            !directories.iter().any(|directory| {
                directory.kind == RootBackingKind::Lower && &directory.path == path
            })
        })
        || !same_backing_directory(upper, work)
        || upper.mount_source != sampled.mount_source()
        || upper.mount_root != sampled.mount_root()
        || upper.mount_device_major != sampled.device_major()
        || upper.mount_device_minor != sampled.device_minor()
        || upper.statx_device_major != sampled.statx_device_major()
        || upper.statx_device_minor != sampled.statx_device_minor()
        || upper.filesystem_type_magic != sampled.filesystem_type_magic()
        || upper.filesystem_id != sampled.filesystem_id()
        || upper.mount_filesystem_type != sampled.filesystem_type_name()
        || directories.iter().any(|directory| {
            directory.mount_source.is_empty()
                || directory.mount_source != upper.mount_source
                || directory.mount_root != upper.mount_root
                || directory.mount_filesystem_type != upper.mount_filesystem_type
                || directory.mount_read_only
                || directory.mount_device_major != upper.mount_device_major
                || directory.mount_device_minor != upper.mount_device_minor
                || directory.statx_mount_id != upper.statx_mount_id
                || directory.statx_device_major != upper.statx_device_major
                || directory.statx_device_minor != upper.statx_device_minor
                || directory.filesystem_type_magic != upper.filesystem_type_magic
                || directory.filesystem_id != upper.filesystem_id
                || directory.statx_mount_id == 0
                || directory.filesystem_id == 0
                || directory.total_bytes == 0
                || directory.free_bytes > directory.total_bytes
                || directory.available_bytes > directory.free_bytes
                || directory.inode_free > directory.inode_total
        })
    {
        return Err(HostError::Identity);
    }
    let backing_digest = crate::launch::resource_probe::filesystem_instance_digest(
        engine.engine_id(),
        engine.docker_root_digest(),
        boot_id.as_bytes(),
        upper.mount_source.as_bytes(),
        upper.mount_filesystem_type.as_bytes(),
        upper.filesystem_type_magic,
        upper.mount_device_major,
        upper.mount_device_minor,
        upper.statx_device_major,
        upper.statx_device_minor,
        upper.filesystem_id,
    );
    let logical_binding = &logical.binding;
    let canonical_record = serde_json::to_string(&serde_json::json!({
        "schema_version": 1,
        "reservation_id": plan.reservation_id(),
        "operation_id": plan.operation_id(),
        "connection_id": journal.connection_id().as_str(),
        "engine_id": engine.engine_id(),
        "docker_root_digest": engine.docker_root_digest(),
        "sample_generation_id": plan.sample_generation_id(),
        "role": role.as_str(),
        "container_id": row.container_id,
        "image_runtime_id": expected_image.runtime_id(),
        "image_source_sha": images.source_sha(),
        "logical": {
            "mount_id": logical_binding.mount_id(),
            "mount_source": row.mount_source,
            "device": logical_binding.device(),
            "statx_device": logical_binding.statx_device(),
            "fs_type_magic": logical_binding.fs_type_magic(),
            "filesystem_id": logical_binding.filesystem_id(),
            "filesystem_digest": logical_binding.filesystem_instance_digest(),
        },
        "backing": row,
    }))
    .map_err(|_| HostError::Frame)?;
    let mapping_digest = lowercase_hex(Sha256::digest(
        [
            b"velnor-root-backing-map-v1\0".as_slice(),
            canonical_record.as_bytes(),
        ]
        .concat(),
    ));
    Ok(super::super::super::VerifiedRootBackingMapping {
        reservation_id: plan.reservation_id(),
        operation_id: plan.operation_id(),
        connection_id: journal.connection_id().as_str().to_owned(),
        engine_id: engine.engine_id().to_owned(),
        docker_root_digest: engine.docker_root_digest().to_owned(),
        sample_generation_id: plan.sample_generation_id().to_owned(),
        role,
        container_id: row.container_id.clone(),
        image_runtime_id: expected_image.runtime_id().to_owned(),
        image_source_sha: images.source_sha().to_owned(),
        logical_mount_id: logical_binding.mount_id(),
        logical_mount_source: row.mount_source.clone(),
        logical_device: logical_binding.device(),
        logical_statx_device: logical_binding.statx_device(),
        logical_fs_type_magic: logical_binding.fs_type_magic(),
        logical_filesystem_id: logical_binding.filesystem_id(),
        logical_filesystem_instance_digest: logical_binding.filesystem_instance_digest().to_owned(),
        backing_mount_source: upper.mount_source.clone(),
        backing_device: (upper.mount_device_major, upper.mount_device_minor),
        backing_statx_device: (upper.statx_device_major, upper.statx_device_minor),
        backing_fs_type_magic: upper.filesystem_type_magic,
        backing_filesystem_id: upper.filesystem_id,
        backing_filesystem_instance_digest: backing_digest,
        canonical_record,
        mapping_digest,
    })
}

fn same_backing_directory(
    left: &velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
    right: &velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
) -> bool {
    left.mount_source == right.mount_source
        && left.mount_root == right.mount_root
        && left.mount_filesystem_type == right.mount_filesystem_type
        && left.mount_device_major == right.mount_device_major
        && left.mount_device_minor == right.mount_device_minor
        && left.statx_mount_id == right.statx_mount_id
        && left.statx_device_major == right.statx_device_major
        && left.statx_device_minor == right.statx_device_minor
        && left.filesystem_type_magic == right.filesystem_type_magic
        && left.filesystem_id == right.filesystem_id
}

pub(super) fn apply_backing_capacity(
    attachments: &mut [MeasuredAttachment],
    mappings: &[super::super::super::VerifiedRootBackingMapping],
    observation: &OwnedCgroupObservation,
) -> Result<(), HostError> {
    for (mapping, role) in mappings
        .iter()
        .zip([StorageRole::RunnerRoot, StorageRole::DindRoot])
    {
        let row = observation
            .root_backings
            .iter()
            .find(|row| row.container_id == mapping.container_id())
            .ok_or(HostError::Identity)?;
        let upper = row
            .directories
            .iter()
            .find(|directory| directory.kind == RootBackingKind::Upper)
            .ok_or(HostError::Identity)?;
        let measured = attachments
            .iter_mut()
            .find(|attachment| attachment.role == role)
            .ok_or(HostError::Identity)?;
        measured.capacity_digest = mapping.backing_filesystem_digest().to_owned();
        measured.capacity_fs_type_magic = upper.filesystem_type_magic;
        measured.total_bytes = upper.total_bytes;
        measured.free_bytes = upper.free_bytes;
        measured.available_bytes = upper.available_bytes;
        measured.inode_total = upper.inode_total;
        measured.inode_free = upper.inode_free;
        measured.inode_available = upper.inode_available;
        measured.inode_policy = if upper.filesystem_type_magic == BTRFS_MAGIC {
            InodePolicy::DynamicBtrfs
        } else {
            InodePolicy::Finite
        };
        measured.filesystem_type_name = upper.mount_filesystem_type.clone();
    }
    Ok(())
}
