//! This module holds input checks for the native cleanup proof adapter.

use velnor_resource_probe::cleanup_observer_wire::CleanupObserverBinding;

use crate::{
    error::HostError,
    journal::{
        StorageCleanupIdentity, StorageCleanupObserverWitness, StorageObserver, StorageRole,
    },
};

pub(super) fn validate_witness(
    identity: &StorageCleanupIdentity,
    witness: &StorageCleanupObserverWitness,
) -> Result<(), HostError> {
    if identity.reservation_id() <= 0
        || identity.operation_id() <= 0
        || identity.engine_id().is_empty()
        || !lower_hex(identity.docker_root_digest(), 64)
        || identity.runner_container_id() == identity.dind_container_id()
        || !lower_hex(identity.instance_binding_digest(), 64)
        || witness.reservation_id() != identity.reservation_id()
        || witness.operation_id() != identity.operation_id()
        || witness.engine_id() != identity.engine_id()
        || witness.docker_root_digest() != identity.docker_root_digest()
        || witness.runner_container_id() != identity.runner_container_id()
        || witness.dind_container_id() != identity.dind_container_id()
        || witness.instance_binding_digest() != identity.instance_binding_digest()
    {
        return Err(HostError::Ownership);
    }
    let binding = witness.binding();
    if binding.expected_guest_boot_id != identity.guest_boot_id() {
        return Err(HostError::Ownership);
    }
    validate_binding(binding)?;
    validate_cgroups(identity, binding)?;
    validate_tmpfs(identity, binding)?;
    validate_volume_attachments(identity, binding)?;
    validate_docker_root(identity, binding)?;
    Ok(())
}

pub(super) fn validate_binding(binding: &CleanupObserverBinding) -> Result<(), HostError> {
    velnor_resource_probe::cleanup_observer_wire::validate_binding(binding)
        .map_err(|_| HostError::Identity)
}

#[cfg(test)]
#[path = "request/tests.rs"]
mod tests;

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_cgroups(
    identity: &StorageCleanupIdentity,
    binding: &CleanupObserverBinding,
) -> Result<(), HostError> {
    let mut paths = [None, None];
    for cgroup in identity.cgroups() {
        let index = match cgroup.role() {
            "runner" if cgroup.container_id() == identity.runner_container_id() => 0,
            "dind" if cgroup.container_id() == identity.dind_container_id() => 1,
            _ => return Err(HostError::Identity),
        };
        if paths[index].replace(cgroup.path()).is_some() {
            return Err(HostError::Identity);
        }
    }
    if paths
        != [
            Some(binding.cgroup_paths[0].as_str()),
            Some(binding.cgroup_paths[1].as_str()),
        ]
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn validate_tmpfs(
    identity: &StorageCleanupIdentity,
    binding: &CleanupObserverBinding,
) -> Result<(), HostError> {
    let keys = [
        (
            StorageRole::SharedRun,
            "runner",
            "/run",
            identity.runner_container_id(),
        ),
        (
            StorageRole::RunnerHome,
            "runner",
            "/home/runner",
            identity.runner_container_id(),
        ),
        (
            StorageRole::RunnerWork,
            "runner",
            "/home/runner/_work",
            identity.runner_container_id(),
        ),
        (
            StorageRole::RunnerTmp,
            "runner",
            "/tmp",
            identity.runner_container_id(),
        ),
        (
            StorageRole::RunnerShm,
            "runner",
            "/dev/shm",
            identity.runner_container_id(),
        ),
        (
            StorageRole::DindData,
            "dind",
            "/var/lib/docker",
            identity.dind_container_id(),
        ),
        (
            StorageRole::DindTmp,
            "dind",
            "/tmp",
            identity.dind_container_id(),
        ),
        (
            StorageRole::DindShm,
            "dind",
            "/dev/shm",
            identity.dind_container_id(),
        ),
    ];
    let mut devices = keys
        .map(|(role, observer, target, container_id)| {
            let mut matching = identity.mounts().iter().filter(|mount| {
                mount.role() == role.as_str()
                    && mount.observer() == observer
                    && mount.target() == target
                    && mount.container_id() == Some(container_id)
            });
            let mount = matching.next().ok_or(HostError::Ownership)?;
            if matching.next().is_some() || mount.device() != mount.statx_device() {
                return Err(HostError::Identity);
            }
            Ok([mount.device().0, mount.device().1])
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    devices.sort_unstable();
    let mut expected = binding.owned_tmpfs_devices.to_vec();
    expected.sort_unstable();
    if devices.len() != 8 || devices != expected {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn validate_docker_root(
    identity: &StorageCleanupIdentity,
    binding: &CleanupObserverBinding,
) -> Result<(), HostError> {
    let mut roots = identity.mounts().iter().filter(|mount| {
        mount.role() == "engine-metadata"
            && mount.observer() == "probe"
            && mount.target() == binding.docker_root.target
    });
    let mount = roots.next().ok_or(HostError::Identity)?;
    let filesystem_id = mount
        .filesystem_id()
        .and_then(|value| u64::from_str_radix(value, 16).ok());
    if roots.next().is_some()
        || binding.docker_root.target != "/velnor/docker-root"
        || binding.docker_root.mount_id != mount.mount_id()
        || binding.docker_root.statx_mount_id != mount.mount_id()
        || binding.docker_root.mount_device != [mount.device().0, mount.device().1]
        || binding.docker_root.statx_device != [mount.statx_device().0, mount.statx_device().1]
        || binding.docker_root.filesystem_type_magic != mount.fs_type_magic()
        || filesystem_id != Some(binding.docker_root.filesystem_id)
        || !mount.read_only()
        || !binding.docker_root.read_only
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn validate_volume_attachments(
    identity: &StorageCleanupIdentity,
    binding: &CleanupObserverBinding,
) -> Result<(), HostError> {
    if identity.volumes().len() != binding.volume_mountpoints.len() {
        return Err(HostError::Ownership);
    }
    for ((role, _), volume) in identity.volumes().iter().zip(&binding.volume_mountpoints) {
        if role.as_str() != volume.role {
            return Err(HostError::Ownership);
        }
        for contract in role.attachments().iter().filter(|item| {
            item.volume_subpath.is_none()
                && !(item.role == StorageRole::EngineMetadata
                    && item.observer != StorageObserver::Probe)
        }) {
            let observer = match contract.observer {
                StorageObserver::Runner => "runner",
                StorageObserver::Dind => "dind",
                StorageObserver::Probe => "probe",
            };
            let container_id = match contract.observer {
                StorageObserver::Runner => Some(identity.runner_container_id()),
                StorageObserver::Dind => Some(identity.dind_container_id()),
                StorageObserver::Probe => None,
            };
            let mut matches = identity.mounts().iter().filter(|mount| {
                mount.role() == role.as_str()
                    && mount.observer() == observer
                    && mount.container_id() == container_id
                    && mount.target() == contract.target
            });
            let mount = matches.next().ok_or(HostError::Ownership)?;
            if matches.next().is_some()
                || mount.source() != contract.source_target
                || mount.read_only() != contract.read_only
                || (*role != StorageRole::EngineMetadata
                    || contract.observer != StorageObserver::Probe)
                    && mount.device() != (volume.mount_device[0], volume.mount_device[1])
                || (*role != StorageRole::EngineMetadata
                    || contract.observer != StorageObserver::Probe)
                    && mount.statx_device() != (volume.statx_device[0], volume.statx_device[1])
            {
                return Err(HostError::Ownership);
            }
        }
    }
    Ok(())
}
