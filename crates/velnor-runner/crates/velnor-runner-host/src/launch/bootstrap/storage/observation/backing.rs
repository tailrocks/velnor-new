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
    context: &RootBackingContext<'_>,
    attachments: &[MeasuredAttachment],
    observation: &OwnedCgroupObservation,
) -> Result<Vec<super::super::super::VerifiedRootBackingMapping>, HostError> {
    let rows = observation.root_backings.as_slice();
    let [runner, dind] = rows else {
        return Err(HostError::Identity);
    };
    let runner_mapping =
        root_backing_mapping(context, attachments, runner, StorageRole::RunnerRoot)?;
    let dind_mapping = root_backing_mapping(context, attachments, dind, StorageRole::DindRoot)?;
    if runner_mapping.backing_filesystem_digest() != dind_mapping.backing_filesystem_digest() {
        return Err(HostError::Identity);
    }
    Ok(vec![runner_mapping, dind_mapping])
}

pub(super) struct RootBackingContext<'a> {
    pub(super) plan: &'a StorageProvisioningPlan,
    pub(super) journal: &'a ConnectionJournal,
    pub(super) engine: &'a SelectedEngineContext,
    pub(super) images: &'a VerifiedWorkerImages,
    pub(super) witness: &'a StartObservationWitness,
}

struct ValidatedBacking<'a> {
    logical: &'a MeasuredAttachment,
    expected_image: &'a crate::launch::VerifiedImageIdentity,
    upper: &'a velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
    backing_digest: String,
}

fn root_backing_mapping(
    context: &RootBackingContext<'_>,
    attachments: &[MeasuredAttachment],
    row: &RootBackingObservation,
    role: StorageRole,
) -> Result<super::super::super::VerifiedRootBackingMapping, HostError> {
    let validated = validate_root_backing(context, attachments, row, role)?;
    build_root_backing_mapping(context, row, role, validated)
}

fn validate_root_backing<'a>(
    context: &'a RootBackingContext<'a>,
    attachments: &'a [MeasuredAttachment],
    row: &'a RootBackingObservation,
    role: StorageRole,
) -> Result<ValidatedBacking<'a>, HostError> {
    let worker_role = match role {
        StorageRole::RunnerRoot => WorkerRole::Runner,
        StorageRole::DindRoot => WorkerRole::Dind,
        _ => return Err(HostError::Identity),
    };
    let logical = attachments
        .iter()
        .find(|item| {
            item.role == role && item.binding.container_id() == Some(row.container_id.as_str())
        })
        .ok_or(HostError::Identity)?;
    let expected_image = match worker_role {
        WorkerRole::Runner => context.images.runner(),
        WorkerRole::Dind => context.images.dind(),
    };
    let directories = row.directories.as_slice();
    let upper = directories
        .iter()
        .find(|item| item.kind == RootBackingKind::Upper)
        .ok_or(HostError::Identity)?;
    let work = directories
        .iter()
        .find(|item| item.kind == RootBackingKind::Work)
        .ok_or(HostError::Identity)?;
    validate_backing_directories(row, upper, work, context.witness)?;
    if row.role != worker_role
        || row.guest_boot_id != context.witness.storage_mount().guest_boot_id()
        || row.mount_id != logical.binding.mount_id()
        || row.mount_source.is_empty()
        || row.filesystem_type != "overlay"
        || row.read_only
        || row.upperdir.is_empty()
        || row.workdir.is_empty()
    {
        return Err(HostError::Identity);
    }
    let backing_digest = crate::launch::resource_probe::filesystem_instance_digest(
        context.engine.engine_id(),
        context.engine.docker_root_digest(),
        context.witness.storage_mount().guest_boot_id().as_bytes(),
        upper.mount_source.as_bytes(),
        upper.mount_filesystem_type.as_bytes(),
        upper.filesystem_type_magic,
        upper.mount_device_major,
        upper.mount_device_minor,
        upper.statx_device_major,
        upper.statx_device_minor,
        upper.filesystem_id,
    );
    Ok(ValidatedBacking {
        logical,
        expected_image,
        upper,
        backing_digest,
    })
}

fn validate_backing_directories(
    row: &RootBackingObservation,
    upper: &velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
    work: &velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
    witness: &StartObservationWitness,
) -> Result<(), HostError> {
    let directories = row.directories.as_slice();
    let lower_count = directories
        .iter()
        .filter(|item| item.kind == RootBackingKind::Lower)
        .count();
    let sampled = witness.storage_mount();
    if lower_count != row.lowerdirs.len()
        || directories.len() != lower_count + 2
        || upper.path != row.upperdir
        || work.path != row.workdir
        || row.lowerdirs.iter().any(|path| {
            !directories
                .iter()
                .any(|item| item.kind == RootBackingKind::Lower && &item.path == path)
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
        || directories
            .iter()
            .any(|item| !directory_matches_upper(item, upper))
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn directory_matches_upper(
    item: &velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
    upper: &velnor_resource_probe::worker_binding::RootBackingDirectoryObservation,
) -> bool {
    !item.mount_source.is_empty()
        && item.mount_source == upper.mount_source
        && item.mount_root == upper.mount_root
        && item.mount_filesystem_type == upper.mount_filesystem_type
        && !item.mount_read_only
        && item.mount_device_major == upper.mount_device_major
        && item.mount_device_minor == upper.mount_device_minor
        && item.statx_mount_id == upper.statx_mount_id
        && item.statx_device_major == upper.statx_device_major
        && item.statx_device_minor == upper.statx_device_minor
        && item.filesystem_type_magic == upper.filesystem_type_magic
        && item.filesystem_id == upper.filesystem_id
        && item.statx_mount_id != 0
        && item.filesystem_id != 0
        && item.total_bytes > 0
        && item.free_bytes <= item.total_bytes
        && item.available_bytes <= item.free_bytes
        && item.inode_free <= item.inode_total
}

fn build_root_backing_mapping(
    context: &RootBackingContext<'_>,
    row: &RootBackingObservation,
    role: StorageRole,
    validated: ValidatedBacking<'_>,
) -> Result<super::super::super::VerifiedRootBackingMapping, HostError> {
    let plan = context.plan;
    let journal = context.journal;
    let engine = context.engine;
    let logical_binding = &validated.logical.binding;
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
        "image_runtime_id": validated.expected_image.runtime_id(),
        "image_source_sha": context.images.source_sha(),
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
        image_runtime_id: validated.expected_image.runtime_id().to_owned(),
        image_source_sha: context.images.source_sha().to_owned(),
        logical_mount_id: logical_binding.mount_id(),
        logical_mount_source: row.mount_source.clone(),
        logical_device: logical_binding.device(),
        logical_statx_device: logical_binding.statx_device(),
        logical_fs_type_magic: logical_binding.fs_type_magic(),
        logical_filesystem_id: logical_binding.filesystem_id(),
        logical_filesystem_instance_digest: logical_binding.filesystem_instance_digest().to_owned(),
        backing_mount_source: validated.upper.mount_source.clone(),
        backing_device: (
            validated.upper.mount_device_major,
            validated.upper.mount_device_minor,
        ),
        backing_statx_device: (
            validated.upper.statx_device_major,
            validated.upper.statx_device_minor,
        ),
        backing_fs_type_magic: validated.upper.filesystem_type_magic,
        backing_filesystem_id: validated.upper.filesystem_id,
        backing_filesystem_instance_digest: validated.backing_digest,
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
        mapping
            .backing_filesystem_digest()
            .clone_into(&mut measured.capacity_digest);
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
        measured
            .filesystem_type_name
            .clone_from(&upper.mount_filesystem_type);
    }
    Ok(())
}
