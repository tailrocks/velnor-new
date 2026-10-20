use crate::error::HostError;
use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageObserver, StorageRole,
};

use super::super::{ObservedStorageAttachment, ObservedStorageRole};

const ATTACHMENTS: [(StorageRole, StorageObserver, &str); 20] = [
    (StorageRole::RunnerRoot, StorageObserver::Runner, "/"),
    (
        StorageRole::RunnerHome,
        StorageObserver::Runner,
        "/home/runner",
    ),
    (
        StorageRole::RunnerHome,
        StorageObserver::Dind,
        "/home/runner",
    ),
    (
        StorageRole::RunnerWork,
        StorageObserver::Runner,
        "/home/runner/_work",
    ),
    (
        StorageRole::RunnerWork,
        StorageObserver::Dind,
        "/home/runner/_work",
    ),
    (StorageRole::RunnerTmp, StorageObserver::Runner, "/tmp"),
    (StorageRole::RunnerShm, StorageObserver::Runner, "/dev/shm"),
    (StorageRole::SharedRun, StorageObserver::Runner, "/run"),
    (StorageRole::SharedRun, StorageObserver::Dind, "/run"),
    (StorageRole::DindRoot, StorageObserver::Dind, "/"),
    (
        StorageRole::DindData,
        StorageObserver::Dind,
        "/var/lib/docker",
    ),
    (StorageRole::DindTmp, StorageObserver::Dind, "/tmp"),
    (StorageRole::DindShm, StorageObserver::Dind, "/dev/shm"),
    (
        StorageRole::EngineMetadata,
        StorageObserver::Probe,
        "/velnor/docker-root",
    ),
    (
        StorageRole::EngineMetadata,
        StorageObserver::Runner,
        "/.velnor-runtime-identity",
    ),
    (
        StorageRole::EngineMetadata,
        StorageObserver::Runner,
        "/opt/velnor/job-facts",
    ),
    (
        StorageRole::EngineMetadata,
        StorageObserver::Runner,
        "/opt/velnor/source-proof/control",
    ),
    (
        StorageRole::EngineMetadata,
        StorageObserver::Dind,
        "/.velnor-runtime-identity",
    ),
    (
        StorageRole::EngineMetadata,
        StorageObserver::Dind,
        "/opt/velnor/job-facts",
    ),
    (
        StorageRole::RunnerHome,
        StorageObserver::Runner,
        "/opt/velnor/source-proof/inbox",
    ),
];
use super::{BTRFS_SUPER_MAGIC, valid_digest};

pub(super) fn validate_roles(roles: &[ObservedStorageRole]) -> Result<(), HostError> {
    const PHYSICAL: [StorageRole; 11] = [
        StorageRole::RunnerRoot,
        StorageRole::RunnerHome,
        StorageRole::RunnerWork,
        StorageRole::RunnerTmp,
        StorageRole::RunnerShm,
        StorageRole::SharedRun,
        StorageRole::DindRoot,
        StorageRole::DindData,
        StorageRole::DindTmp,
        StorageRole::DindShm,
        StorageRole::EngineMetadata,
    ];
    if roles.len() != PHYSICAL.len()
        || PHYSICAL.iter().any(|role| {
            roles
                .iter()
                .filter(|observed| observed.role == *role)
                .count()
                != 1
        })
    {
        return Err(HostError::Journal);
    }
    for observed in roles {
        validate_role(observed)?;
    }
    Ok(())
}

fn validate_role(role: &ObservedStorageRole) -> Result<(), HostError> {
    let contract = role.role.contract().ok_or(HostError::Journal)?;
    let (total, free, available) = role.byte_counts();
    let (inode_total, inode_free, inode_available) = role.inode_counts();
    let (charge_bytes, charge_inodes, retained_memory_bytes) = role.charges();
    let (owned_usage_bytes, owned_usage_inodes) = role.owned_usage();
    let bytes_cover = available
        .checked_add(owned_usage_bytes)
        .is_some_and(|capacity| capacity >= charge_bytes);
    let inode_shape = match role.inode_policy() {
        StorageInodePolicy::Finite => match (inode_total, inode_free, inode_available) {
            (Some(total), Some(free), Some(available)) => {
                total > 0
                    && free <= total
                    && available <= free
                    && charge_inodes > 0
                    && available
                        .checked_add(owned_usage_inodes)
                        .is_some_and(|capacity| capacity >= charge_inodes)
            }
            _ => false,
        },
        StorageInodePolicy::DynamicBtrfs => {
            role.medium() == StorageMedium::Disk
                && role.filesystem_kind() == StorageFilesystemKind::Btrfs
                && charge_inodes > 0
                && owned_usage_inodes == 0
                && inode_total == Some(0)
                && inode_free == Some(0)
                && inode_available == Some(0)
        }
        StorageInodePolicy::Unknown => false,
    };
    let medium_shape = match role.medium() {
        StorageMedium::Tmpfs => {
            role.filesystem_kind() == StorageFilesystemKind::Tmpfs
                && role.inode_policy() == StorageInodePolicy::Finite
                && retained_memory_bytes == charge_bytes
        }
        StorageMedium::Disk => {
            role.filesystem_kind() != StorageFilesystemKind::Tmpfs && retained_memory_bytes == 0
        }
    };
    if !valid_digest(role.pool_id())
        || !valid_digest(role.filesystem_instance_digest())
        || role.medium() != contract.medium
        || total == 0
        || free > total
        || available > free
        || charge_bytes == 0
        || owned_usage_bytes > charge_bytes
        || !bytes_cover
        || !inode_shape
        || !medium_shape
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) fn validate_attachments(
    roles: &[ObservedStorageRole],
    attachments: &[ObservedStorageAttachment],
    runner_container_id: &str,
    dind_container_id: &str,
) -> Result<(), HostError> {
    if !has_expected_attachment_set(attachments) {
        return Err(HostError::Journal);
    }
    validate_engine_metadata_views(roles, attachments)?;
    for item in attachments {
        validate_attachment(roles, item, runner_container_id, dind_container_id)?;
    }
    Ok(())
}

fn validate_engine_metadata_views(
    roles: &[ObservedStorageRole],
    attachments: &[ObservedStorageAttachment],
) -> Result<(), HostError> {
    let capacity = unique_role(roles, StorageRole::EngineMetadata)?;
    let probe = unique_attachment(
        attachments,
        StorageRole::EngineMetadata,
        StorageObserver::Probe,
        None,
        "/velnor/docker-root",
    )?;
    if probe.filesystem_instance_digest != capacity.filesystem_instance_digest {
        return Err(HostError::Journal);
    }
    let views = attachments
        .iter()
        .filter(|item| {
            item.role == StorageRole::EngineMetadata && item.observer != StorageObserver::Probe
        })
        .collect::<Vec<_>>();
    if views.len() != 5 {
        return Err(HostError::Journal);
    }
    let root = metadata_volume_root(views[0])?;
    let first = views[0];
    if views.iter().any(|item| {
        metadata_volume_root(item).ok() != Some(root)
            || item.device() != first.device()
            || item.statx_device() != first.statx_device()
            || item.fs_type_magic != first.fs_type_magic
            || item.filesystem_id != first.filesystem_id
            || item.filesystem_instance_digest != first.filesystem_instance_digest
    }) {
        return Err(HostError::Identity);
    }
    validate_metadata_view_pair(&views, "/.velnor-runtime-identity", "runtime-identity")?;
    validate_metadata_view_pair(&views, "/opt/velnor/job-facts", "job-facts")?;
    Ok(())
}

fn unique_role(
    roles: &[ObservedStorageRole],
    expected: StorageRole,
) -> Result<&ObservedStorageRole, HostError> {
    let mut matching = roles.iter().filter(|role| role.role == expected);
    let role = matching.next().ok_or(HostError::Journal)?;
    if matching.next().is_some() {
        return Err(HostError::Journal);
    }
    Ok(role)
}

fn unique_attachment<'a>(
    attachments: &'a [ObservedStorageAttachment],
    role: StorageRole,
    observer: StorageObserver,
    container_id: Option<&str>,
    target: &str,
) -> Result<&'a ObservedStorageAttachment, HostError> {
    let mut matching = attachments.iter().filter(|item| {
        item.role == role
            && item.observer == observer
            && item.container_id.as_deref() == container_id
            && item.target == target
    });
    let item = matching.next().ok_or(HostError::Journal)?;
    if matching.next().is_some() {
        return Err(HostError::Journal);
    }
    Ok(item)
}

fn metadata_volume_root(item: &ObservedStorageAttachment) -> Result<&str, HostError> {
    let subpath = item.volume_subpath.as_deref().ok_or(HostError::Identity)?;
    let suffix = format!("/{subpath}");
    let root = item
        .mount_root()
        .strip_suffix(&suffix)
        .filter(|root| !root.is_empty())
        .ok_or(HostError::Identity)?;
    Ok(root)
}

fn validate_metadata_view_pair(
    views: &[&ObservedStorageAttachment],
    target: &str,
    subpath: &str,
) -> Result<(), HostError> {
    let runner = views
        .iter()
        .copied()
        .find(|item| {
            item.observer == StorageObserver::Runner
                && item.target == target
                && item.volume_subpath.as_deref() == Some(subpath)
        })
        .ok_or(HostError::Identity)?;
    let dind = views
        .iter()
        .copied()
        .find(|item| {
            item.observer == StorageObserver::Dind
                && item.target == target
                && item.volume_subpath.as_deref() == Some(subpath)
        })
        .ok_or(HostError::Identity)?;
    if runner.mount_root() != dind.mount_root()
        || runner.statx_inode != dind.statx_inode
        || runner.device() != dind.device()
        || runner.statx_device() != dind.statx_device()
        || runner.fs_type_magic != dind.fs_type_magic
        || runner.filesystem_id != dind.filesystem_id
        || runner.filesystem_instance_digest != dind.filesystem_instance_digest
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn has_expected_attachment_set(attachments: &[ObservedStorageAttachment]) -> bool {
    attachments.len() == ATTACHMENTS.len()
        && ATTACHMENTS.iter().all(|(role, observer, target)| {
            attachments
                .iter()
                .filter(|item| {
                    item.role == *role && item.observer == *observer && item.target == *target
                })
                .count()
                == 1
        })
}

fn validate_attachment(
    roles: &[ObservedStorageRole],
    item: &ObservedStorageAttachment,
    runner_container_id: &str,
    dind_container_id: &str,
) -> Result<(), HostError> {
    let contract = item
        .role
        .attachments()
        .iter()
        .find(|contract| contract.observer == item.observer && contract.target == item.target)
        .ok_or(HostError::Journal)?;
    let expected_container = match item.observer {
        StorageObserver::Runner => Some(runner_container_id),
        StorageObserver::Dind => Some(dind_container_id),
        StorageObserver::Probe => None,
    };
    let observed_role = roles
        .iter()
        .find(|role| role.role == item.role)
        .ok_or(HostError::Journal)?;
    let device_matches = item.fs_type_magic == BTRFS_SUPER_MAGIC
        || (item.device_major, item.device_minor)
            == (item.statx_device_major, item.statx_device_minor);
    if item.source_target.as_deref() != contract.source_target
        || item.volume_subpath.as_deref() != contract.volume_subpath
        || !valid_mount_root(item.mount_root(), contract.volume_subpath)
        || item.read_only != contract.read_only
        || item.container_id.as_deref() != expected_container
        || item.mount_id == 0
        || item.statx_inode == 0
        || item.fs_type_magic == 0
        || item.filesystem_id == 0
        || !device_matches
        || !valid_digest(&item.filesystem_instance_digest)
        || (item.role != StorageRole::EngineMetadata || item.observer == StorageObserver::Probe)
            && item.filesystem_instance_digest != observed_role.filesystem_instance_digest
    {
        return Err(HostError::Journal);
    }
    Ok(())
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

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
