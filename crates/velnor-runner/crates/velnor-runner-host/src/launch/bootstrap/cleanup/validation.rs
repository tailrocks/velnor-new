//! This module compares the full guest resource list with the cleanup witness in the journal.

use velnor_resource_probe::cleanup_observer_wire::{CleanupObserverBinding, CleanupObserverRecord};

use crate::{
    error::HostError,
    journal::{StorageCleanupIdentity, StorageCleanupObserverWitness},
};

use super::production::ObservedDockerCleanupAbsence;
use crate::launch::resource_probe::VerifiedCleanupObserverRecord;

pub(super) fn validate_observations(
    identity: &StorageCleanupIdentity,
    witness: &StorageCleanupObserverWitness,
    docker: &ObservedDockerCleanupAbsence,
    observer: &VerifiedCleanupObserverRecord,
) -> Result<(), HostError> {
    let binding = witness.binding();
    let record = observer.record();
    if observer.reservation_id() != witness.reservation_id()
        || observer.engine_id() != witness.engine_id()
        || observer.docker_root_digest() != witness.docker_root_digest()
        || record.operation_id != observer.operation_id()
        || record.binding_sha256 != witness.binding_sha256()
        || docker.reservation_id() != witness.reservation_id()
        || docker.operation_id() != witness.operation_id()
        || docker.engine_id() != witness.engine_id()
        || docker.docker_root_digest() != witness.docker_root_digest()
        || docker.runner_container_id() != witness.runner_container_id()
        || docker.dind_container_id() != witness.dind_container_id()
        || docker.volume_inventory() != identity.volumes()
        || binding
            .volume_mountpoints
            .iter()
            .map(|volume| volume.role.as_str())
            .ne(identity.volumes().iter().map(|(role, _)| role.as_str()))
    {
        return Err(HostError::Ownership);
    }
    validate_record(
        record,
        binding,
        observer.operation_id(),
        witness.binding_sha256(),
    )
}

fn validate_record(
    record: &CleanupObserverRecord,
    binding: &CleanupObserverBinding,
    operation_id: &str,
    binding_sha256: &str,
) -> Result<(), HostError> {
    let volumes = binding
        .volume_mountpoints
        .iter()
        .map(|volume| volume.mountpoint.clone())
        .collect::<Vec<_>>();
    let namespaces = [&binding.runner, &binding.dind]
        .into_iter()
        .map(
            |process| velnor_resource_probe::cleanup_observer_wire::NamespaceIdentity {
                device: process.namespace_device,
                inode: process.namespace_inode,
            },
        )
        .collect::<Vec<_>>();
    if record.schema_version != 1
        || record.operation_id != operation_id
        || record.binding_sha256 != binding_sha256
        || record.guest_boot_id.as_deref() != Some(binding.expected_guest_boot_id.as_str())
        || !record.docker_root.as_ref().is_some_and(|root| {
            velnor_resource_probe::cleanup_observer_wire::docker_root_matches(
                &binding.docker_root,
                root,
            )
        })
        || !record.complete
        || !record.owned_references_absent
        || !record.issues.is_empty()
        || record.owned_reference_count != 0
        || record.absent_volume_mountpoints != volumes
        || record.absent_tmpfs_devices.as_slice() != binding.owned_tmpfs_devices.as_slice()
        || record.absent_mount_namespaces != namespaces
        || record.absent_cgroup_paths.as_ref() != Some(&binding.cgroup_paths)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[cfg(test)]
#[path = "validation/tests.rs"]
mod tests;
