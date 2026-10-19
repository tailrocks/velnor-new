use super::capacity::valid_digest;
use crate::{
    error::HostError,
    journal::{StorageObserver, StorageProvisioningPlan, StorageRole},
    launch::{SelectedEngineContext, bootstrap::ObservedStorageAttachment},
};
use velnor_resource_probe::{
    InodePolicy,
    worker_binding::{AttachmentRole, OwnedCgroupObservation, SelfStorageObservation},
};

const BTRFS_MAGIC: u64 = 0x9123_683e;

pub(super) fn validate_plan(
    plan: &StorageProvisioningPlan,
    reservation_id: i64,
    operation_id: i64,
    sample_generation_id: &str,
    engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    if plan.reservation_id() != reservation_id
        || plan.operation_id() != operation_id
        || plan.sample_generation_id() != sample_generation_id
        || plan.engine_id() != engine.engine_id()
        || plan.docker_root_digest() != engine.docker_root_digest()
        || plan.roles().len() != StorageRole::PHYSICAL.len()
        || plan.roles().iter().any(|role| {
            role.planned_filesystem_instance_digest()
                .is_some_and(|digest| !valid_digest(digest))
        })
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn validate_guest_identity(
    runner: &SelfStorageObservation,
    dind: &SelfStorageObservation,
    expected_boot_id: &str,
) -> Result<(), HostError> {
    if runner.guest_boot_id != expected_boot_id
        || dind.guest_boot_id != expected_boot_id
        || runner.operation_id == dind.operation_id
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) struct MeasuredAttachment {
    pub(in crate::launch::bootstrap::storage::observation) binding: ObservedStorageAttachment,
    pub(in crate::launch::bootstrap::storage::observation) role: StorageRole,
    pub(in crate::launch::bootstrap::storage::observation) capacity_digest: String,
    pub(in crate::launch::bootstrap::storage::observation) capacity_fs_type_magic: u64,
    pub(in crate::launch::bootstrap::storage::observation) total_bytes: u64,
    pub(in crate::launch::bootstrap::storage::observation) free_bytes: u64,
    pub(in crate::launch::bootstrap::storage::observation) available_bytes: u64,
    pub(in crate::launch::bootstrap::storage::observation) inode_total: u64,
    pub(in crate::launch::bootstrap::storage::observation) inode_free: u64,
    pub(in crate::launch::bootstrap::storage::observation) inode_available: u64,
    pub(in crate::launch::bootstrap::storage::observation) inode_policy: InodePolicy,
    pub(in crate::launch::bootstrap::storage::observation) filesystem_type_name: String,
}

pub(super) fn worker_attachments(
    observation: &SelfStorageObservation,
    container_id: &str,
    observer: StorageObserver,
    engine: &SelectedEngineContext,
) -> Result<Vec<MeasuredAttachment>, HostError> {
    observation
        .attachments
        .iter()
        .map(|attachment| {
            let role = storage_role(attachment.role)?;
            let contract = role
                .attachments()
                .iter()
                .find(|contract| {
                    contract.observer == observer && contract.target == attachment.target
                })
                .ok_or(HostError::Ownership)?;
            if attachment.read_only != contract.read_only
                || attachment.mount_id == 0
                || attachment.statx_inode == 0
                || attachment.filesystem_id == 0
                || attachment.total_bytes == 0
                || attachment.free_bytes > attachment.total_bytes
                || attachment.available_bytes > attachment.free_bytes
                || attachment.mount_source.is_empty()
                || attachment.mount_source.len() > 1024
                || attachment
                    .mount_source
                    .bytes()
                    .any(|byte| byte.is_ascii_control())
                || attachment.mount_root.is_empty()
                || !attachment.mount_root.starts_with('/')
                || attachment.mount_root.len() > 1024
                || attachment
                    .mount_root
                    .bytes()
                    .any(|byte| byte.is_ascii_control())
                || !valid_mount_root(&attachment.mount_root, contract.volume_subpath)
                || (attachment.filesystem_type_magic != BTRFS_MAGIC
                    && (attachment.mount_device_major, attachment.mount_device_minor)
                        != (attachment.statx_device_major, attachment.statx_device_minor))
            {
                return Err(HostError::Identity);
            }
            let filesystem_instance_digest =
                crate::launch::resource_probe::filesystem_instance_digest(
                    engine.engine_id(),
                    engine.docker_root_digest(),
                    observation.guest_boot_id.as_bytes(),
                    attachment.mount_source.as_bytes(),
                    attachment.filesystem_type_name.as_bytes(),
                    attachment.filesystem_type_magic,
                    attachment.mount_device_major,
                    attachment.mount_device_minor,
                    attachment.statx_device_major,
                    attachment.statx_device_minor,
                    attachment.filesystem_id,
                );
            let capacity_digest = filesystem_instance_digest.clone();
            let binding = ObservedStorageAttachment {
                role,
                observer,
                container_id: Some(container_id.to_owned()),
                target: attachment.target.clone(),
                source_target: contract.source_target.map(str::to_owned),
                volume_subpath: contract.volume_subpath.map(str::to_owned),
                mount_root: attachment.mount_root.clone(),
                read_only: attachment.read_only,
                mount_id: attachment.mount_id,
                statx_inode: attachment.statx_inode,
                device_major: attachment.mount_device_major,
                device_minor: attachment.mount_device_minor,
                statx_device_major: attachment.statx_device_major,
                statx_device_minor: attachment.statx_device_minor,
                fs_type_magic: attachment.filesystem_type_magic,
                filesystem_id: attachment.filesystem_id,
                filesystem_instance_digest,
            };
            Ok(MeasuredAttachment {
                role,
                capacity_digest,
                capacity_fs_type_magic: attachment.filesystem_type_magic,
                binding,
                total_bytes: attachment.total_bytes,
                free_bytes: attachment.free_bytes,
                available_bytes: attachment.available_bytes,
                inode_total: attachment.inode_total,
                inode_free: attachment.inode_free,
                inode_available: attachment.inode_available,
                inode_policy: attachment.inode_policy,
                filesystem_type_name: attachment.filesystem_type_name.clone(),
            })
        })
        .collect()
}

fn storage_role(role: AttachmentRole) -> Result<StorageRole, HostError> {
    match role {
        AttachmentRole::RunnerRoot => Ok(StorageRole::RunnerRoot),
        AttachmentRole::RunnerHome => Ok(StorageRole::RunnerHome),
        AttachmentRole::RunnerWork => Ok(StorageRole::RunnerWork),
        AttachmentRole::RunnerTmp => Ok(StorageRole::RunnerTmp),
        AttachmentRole::RunnerShm => Ok(StorageRole::RunnerShm),
        AttachmentRole::SharedRun => Ok(StorageRole::SharedRun),
        AttachmentRole::DindRoot => Ok(StorageRole::DindRoot),
        AttachmentRole::DindData => Ok(StorageRole::DindData),
        AttachmentRole::DindTmp => Ok(StorageRole::DindTmp),
        AttachmentRole::DindShm => Ok(StorageRole::DindShm),
        AttachmentRole::SourceProofInbox => Ok(StorageRole::RunnerHome),
        AttachmentRole::JobFacts
        | AttachmentRole::RuntimeIdentity
        | AttachmentRole::SourceProofControl
        | AttachmentRole::EngineMetadata => Ok(StorageRole::EngineMetadata),
    }
}

fn valid_mount_root(value: &str, volume_subpath: Option<&str>) -> bool {
    if value == "/" {
        return volume_subpath.is_none();
    }
    if value.is_empty()
        || value.len() > 1024
        || !value.starts_with('/')
        || value.bytes().any(|byte| byte.is_ascii_control())
        || value
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return false;
    }
    volume_subpath.is_none_or(|subpath| {
        value
            .strip_suffix(subpath)
            .is_some_and(|prefix| prefix.ends_with('/') && !prefix.is_empty())
    })
}

pub(super) fn probe_attachment(
    observation: &OwnedCgroupObservation,
    engine: &SelectedEngineContext,
) -> Result<MeasuredAttachment, HostError> {
    let mount = &observation.engine_metadata;
    if mount.target != "/velnor/docker-root"
        || mount.mount_source.is_empty()
        || mount.statx_inode == 0
        || mount.mount_id == 0
        || mount.filesystem_id == 0
        || mount.free_bytes > mount.total_bytes
        || mount.available_bytes > mount.free_bytes
        || !mount.read_only
    {
        return Err(HostError::Identity);
    }
    let digest = crate::launch::resource_probe::filesystem_instance_digest(
        engine.engine_id(),
        engine.docker_root_digest(),
        observation.guest_boot_id.as_bytes(),
        mount.mount_source.as_bytes(),
        mount.filesystem_type_name.as_bytes(),
        mount.filesystem_type_magic,
        mount.mount_device_major,
        mount.mount_device_minor,
        mount.statx_device_major,
        mount.statx_device_minor,
        mount.filesystem_id,
    );
    let binding = ObservedStorageAttachment {
        role: StorageRole::EngineMetadata,
        observer: StorageObserver::Probe,
        container_id: None,
        target: mount.target.clone(),
        source_target: Some("/var/lib/docker".to_owned()),
        volume_subpath: None,
        mount_root: mount.mount_root.clone(),
        read_only: true,
        mount_id: mount.mount_id,
        statx_inode: mount.statx_inode,
        device_major: mount.mount_device_major,
        device_minor: mount.mount_device_minor,
        statx_device_major: mount.statx_device_major,
        statx_device_minor: mount.statx_device_minor,
        fs_type_magic: mount.filesystem_type_magic,
        filesystem_id: mount.filesystem_id,
        filesystem_instance_digest: digest.clone(),
    };
    Ok(MeasuredAttachment {
        role: StorageRole::EngineMetadata,
        capacity_digest: digest,
        capacity_fs_type_magic: mount.filesystem_type_magic,
        binding,
        total_bytes: mount.total_bytes,
        free_bytes: mount.free_bytes,
        available_bytes: mount.available_bytes,
        inode_total: mount.inode_total,
        inode_free: mount.inode_free,
        inode_available: mount.inode_available,
        inode_policy: mount.inode_policy,
        filesystem_type_name: mount.filesystem_type_name.clone(),
    })
}
