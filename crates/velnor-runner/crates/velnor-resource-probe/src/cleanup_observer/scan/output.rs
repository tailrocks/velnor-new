use crate::cleanup_observation::GuestDockerRootIdentity;

use super::super::{
    CleanupDockerRootIdentity, CleanupObserverRecord, CleanupObserverRequest, NamespaceIdentity,
};
use crate::cleanup_observer_wire::docker_root_matches;

pub(super) struct ScanEvidence {
    pub guest_boot_id: Option<String>,
    pub docker_root: Option<GuestDockerRootIdentity>,
    pub issues: Vec<String>,
    pub task_count: u32,
    pub descriptor_count: u32,
    pub namespace_count: u32,
    pub owned_reference_count: u32,
}

pub(super) fn record(
    request: CleanupObserverRequest,
    evidence: ScanEvidence,
) -> CleanupObserverRecord {
    let complete = evidence.issues.is_empty();
    CleanupObserverRecord {
        schema_version: 1,
        operation_id: request.operation_id,
        binding_sha256: request.binding_sha256,
        guest_boot_id: evidence.guest_boot_id,
        docker_root: evidence.docker_root.map(wire_root),
        complete,
        owned_references_absent: complete && evidence.owned_reference_count == 0,
        issues: evidence.issues,
        task_count: evidence.task_count,
        descriptor_count: evidence.descriptor_count,
        namespace_count: evidence.namespace_count,
        owned_reference_count: evidence.owned_reference_count,
        absent_volume_mountpoints: absence_list(
            complete && evidence.owned_reference_count == 0,
            request
                .binding
                .volume_mountpoints
                .iter()
                .map(|volume| volume.mountpoint.clone())
                .collect(),
        ),
        absent_tmpfs_devices: absence_list(
            complete && evidence.owned_reference_count == 0,
            request.binding.owned_tmpfs_devices.to_vec(),
        ),
        absent_mount_namespaces: absence_list(
            complete && evidence.owned_reference_count == 0,
            [request.binding.runner, request.binding.dind]
                .iter()
                .map(|process| NamespaceIdentity {
                    device: process.namespace_device,
                    inode: process.namespace_inode,
                })
                .collect(),
        ),
        absent_cgroup_paths: (complete && evidence.owned_reference_count == 0)
            .then_some(request.binding.cgroup_paths),
    }
}

fn wire_root(root: GuestDockerRootIdentity) -> CleanupDockerRootIdentity {
    CleanupDockerRootIdentity {
        target: root.target,
        mount_id: root.mount_id,
        statx_mount_id: root.statx_mount_id,
        mount_root: root.mount_root,
        mount_device: root.mount_device,
        statx_device: root.statx_device,
        filesystem_type_magic: root.filesystem_type_magic,
        filesystem_id: root.filesystem_id,
        read_only: root.read_only,
    }
}

pub(super) fn guest_root_matches(
    guest: &GuestDockerRootIdentity,
    binding: &CleanupDockerRootIdentity,
) -> bool {
    docker_root_matches(binding, &wire_root(guest.clone()))
}

fn absence_list<T>(complete: bool, values: Vec<T>) -> Vec<T> {
    if complete { values } else { Vec::new() }
}
