use std::collections::HashMap;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, StorageProvisioningPlan, StorageRole},
    launch::{
        SelectedEngineContext,
        bootstrap::{ObservedStorageAttachment, PreparedAcquiredBootstrap},
    },
    worker::storage_initialization::VerifiedStorageInitializationReady,
};
use bollard::models::{Volume, VolumeScopeEnum};

pub(super) fn object_for_role(
    prepared: &PreparedAcquiredBootstrap,
    role: StorageRole,
) -> Result<&crate::launch::bootstrap::containers::VerifiedCreatedStorageObject, HostError> {
    if role == StorageRole::EngineMetadata {
        return Ok(prepared.metadata());
    }
    let mut found = prepared
        .storage()
        .objects()
        .iter()
        .filter(|item| item.role() == role);
    let object = found.next().ok_or(HostError::Ownership)?;
    if found.next().is_some() {
        return Err(HostError::Ownership);
    }
    Ok(object)
}

pub(super) fn validate_object_identity(
    object: &crate::launch::bootstrap::containers::VerifiedCreatedStorageObject,
    plan: &StorageProvisioningPlan,
    engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    if object.reservation_id() != plan.reservation_id()
        || object.operation_id() != plan.operation_id()
        || object.engine_id() != engine.engine_id()
        || object.docker_root_digest() != engine.docker_root_digest()
        || object.docker_name().is_empty()
        || object.docker_id() != object.docker_name()
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) fn verify_volume(
    volume: &Volume,
    name: &str,
    role: StorageRole,
    journal: &ConnectionJournal,
    plan: &StorageProvisioningPlan,
) -> Result<(), HostError> {
    let labels = HashMap::from([
        (
            "velnor.connection".to_owned(),
            journal.connection_id().as_str().to_owned(),
        ),
        (
            "velnor.reservation".to_owned(),
            plan.reservation_id().to_string(),
        ),
        (
            "velnor.operation".to_owned(),
            plan.operation_id().to_string(),
        ),
        ("velnor.role".to_owned(), role.as_str().to_owned()),
    ]);
    let options = if role == StorageRole::EngineMetadata {
        HashMap::new()
    } else {
        let charge = plan
            .roles()
            .iter()
            .find(|row| row.role() == role)
            .ok_or(HostError::Identity)?;
        HashMap::from([
            ("type".to_owned(), "tmpfs".to_owned()),
            ("device".to_owned(), "tmpfs".to_owned()),
            (
                "o".to_owned(),
                format!(
                    "size={},nr_inodes={},nosuid,nodev",
                    charge.charge_bytes(),
                    charge.charge_inodes()
                ),
            ),
        ])
    };
    if journal.connection_id().as_str().is_empty()
        || volume.name != name
        || volume.driver != "local"
        || volume.options != options
        || volume.labels != labels
        || volume.scope != Some(VolumeScopeEnum::LOCAL)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn volume_root_attachment<'a>(
    attachments: &'a [super::super::super::worker::MeasuredAttachment],
    role: StorageRole,
    runner_id: &str,
    dind_id: &str,
) -> Result<&'a ObservedStorageAttachment, HostError> {
    let (observer, container, target) = match role {
        StorageRole::SharedRun => (crate::journal::StorageObserver::Runner, runner_id, "/run"),
        StorageRole::RunnerHome => (
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/home/runner",
        ),
        StorageRole::RunnerWork => (
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/home/runner/_work",
        ),
        StorageRole::DindData => (
            crate::journal::StorageObserver::Dind,
            dind_id,
            "/var/lib/docker",
        ),
        StorageRole::EngineMetadata => (
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/opt/velnor/job-facts",
        ),
        _ => return Err(HostError::Identity),
    };
    let mut rows = attachments.iter().filter(|item| {
        let row = &item.binding;
        row.role() == role
            && row.observer() == observer
            && row.target() == target
            && row.container_id().unwrap_or_default() == container
    });
    let root = &rows.next().ok_or(HostError::Identity)?.binding;
    let valid_subpath = if role == StorageRole::EngineMetadata {
        root.volume_subpath() == Some("job-facts") && root.read_only()
    } else {
        root.volume_subpath().is_none()
    };
    if rows.next().is_some() || !valid_subpath || root.statx_inode() == 0 {
        return Err(HostError::Identity);
    }
    if role == StorageRole::SharedRun {
        let other = attachments
            .iter()
            .find(|item| {
                let row = &item.binding;
                row.role() == role
                    && row.observer() == crate::journal::StorageObserver::Dind
                    && row.container_id() == Some(dind_id)
                    && row.target() == target
            })
            .ok_or(HostError::Identity)?;
        let other = &other.binding;
        if root.device() != other.device()
            || root.statx_device() != other.statx_device()
            || root.statx_inode() != other.statx_inode()
            || root.filesystem_id() != other.filesystem_id()
            || root.fs_type_magic() != other.fs_type_magic()
            || root.mount_root() != other.mount_root()
        {
            return Err(HostError::Identity);
        }
    }
    Ok(root)
}

pub(super) struct VolumeIdentity {
    pub(super) mount_device: [u32; 2],
    pub(super) statx_device: [u32; 2],
    pub(super) inode: u64,
}

pub(super) fn volume_identity(
    attachments: &[super::super::super::worker::MeasuredAttachment],
    role: StorageRole,
    runner_id: &str,
    dind_id: &str,
    initialization: &VerifiedStorageInitializationReady,
) -> Result<VolumeIdentity, HostError> {
    let root = volume_root_attachment(attachments, role, runner_id, dind_id)?;
    if role != StorageRole::EngineMetadata {
        return Ok(VolumeIdentity {
            mount_device: [root.device().0, root.device().1],
            statx_device: [root.statx_device().0, root.statx_device().1],
            inode: root.statx_inode(),
        });
    }
    let metadata = initialization.metadata_root();
    let job_facts = initialization.job_facts();
    if metadata.mount_id() == 0
        || metadata.inode() == 0
        || job_facts.parent_mount_id() != metadata.mount_id()
        || job_facts.device() != metadata.device()
        || root.device() != metadata.device()
        || root.statx_device() != metadata.device()
    {
        return Err(HostError::Identity);
    }
    Ok(VolumeIdentity {
        mount_device: [root.device().0, root.device().1],
        statx_device: [metadata.device().0, metadata.device().1],
        inode: metadata.inode(),
    })
}

pub(super) fn tmpfs_devices(
    attachments: &[super::super::super::worker::MeasuredAttachment],
    runner_id: &str,
    dind_id: &str,
) -> Result<[[u32; 2]; 8], HostError> {
    let keys = [
        (
            StorageRole::SharedRun,
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/run",
        ),
        (
            StorageRole::RunnerHome,
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/home/runner",
        ),
        (
            StorageRole::RunnerWork,
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/home/runner/_work",
        ),
        (
            StorageRole::RunnerTmp,
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/tmp",
        ),
        (
            StorageRole::RunnerShm,
            crate::journal::StorageObserver::Runner,
            runner_id,
            "/dev/shm",
        ),
        (
            StorageRole::DindData,
            crate::journal::StorageObserver::Dind,
            dind_id,
            "/var/lib/docker",
        ),
        (
            StorageRole::DindTmp,
            crate::journal::StorageObserver::Dind,
            dind_id,
            "/tmp",
        ),
        (
            StorageRole::DindShm,
            crate::journal::StorageObserver::Dind,
            dind_id,
            "/dev/shm",
        ),
    ];
    keys.map(|(role, observer, container, target)| {
        let row = attachments
            .iter()
            .find(|item| {
                let row = &item.binding;
                row.role() == role
                    && row.observer() == observer
                    && row.container_id() == Some(container)
                    && row.target() == target
            })
            .ok_or(HostError::Identity)?;
        let row = &row.binding;
        if row.statx_inode() == 0 {
            return Err(HostError::Identity);
        }
        Ok([row.device().0, row.device().1])
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?
    .try_into()
    .map_err(|_| HostError::Identity)
}
