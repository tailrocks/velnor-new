// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_resource_probe::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord, CleanupVolume,
    NamespaceIdentity, ProcessIdentity, validate_cleanup_observer_record,
};

fn sample_cleanup_binding(
    filesystem_type_magic: u64,
    mount_device: [u32; 2],
    statx_device: [u32; 2],
) -> CleanupObserverBinding {
    let proc_id = |pid, namespace_inode| ProcessIdentity {
        pid,
        start_ticks: 100,
        namespace_inode,
        namespace_device: [0, 4],
    };
    let vol = |(role, inode): (&str, u64)| CleanupVolume {
        role: role.to_owned(),
        mountpoint: format!("/var/lib/docker/volumes/{role}/_data"),
        mount_device,
        statx_device,
        inode,
    };
    let roles = [
        ("shared-run", 101),
        ("runner-home", 102),
        ("runner-work", 103),
        ("dind-data", 104),
        ("engine-metadata", 105),
    ];
    CleanupObserverBinding {
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        metadata_job_facts_inode: Some(500),
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_root: "/docker".to_owned(),
            mount_id: 90,
            statx_mount_id: 90,
            mount_device,
            statx_device,
            filesystem_type_magic,
            filesystem_id: 77,
            read_only: true,
        },
        daemon: proc_id(10, 4001),
        runner: proc_id(20, 4002),
        dind: proc_id(30, 4003),
        cgroup_paths: ["1", "2"].map(|ch| format!("/docker/{}", ch.repeat(64))),
        volume_mountpoints: roles.into_iter().map(vol).collect(),
        owned_tmpfs_devices: [51, 52, 53, 54, 55, 56, 57, 58].map(|minor| [0, minor]),
    }
}

#[test]
fn cleanup_record_requires_absent_references_and_matching_binding() {
    let binding = sample_cleanup_binding(0x9123_683e, [0, 38], [0, 41]);
    let op = "a".repeat(32);
    let digest = "b".repeat(64);
    let record = CleanupObserverRecord {
        schema_version: 1,
        operation_id: op.clone(),
        binding_sha256: digest.clone(),
        guest_boot_id: Some(binding.expected_guest_boot_id.clone()),
        docker_root: Some(binding.docker_root.clone()),
        complete: true,
        owned_references_absent: true,
        issues: Vec::new(),
        task_count: 10,
        descriptor_count: 20,
        namespace_count: 2,
        owned_reference_count: 0,
        absent_volume_mountpoints: binding
            .volume_mountpoints
            .iter()
            .map(|v| v.mountpoint.clone())
            .collect(),
        absent_tmpfs_devices: binding.owned_tmpfs_devices.to_vec(),
        absent_mount_namespaces: [4002, 4003]
            .map(|inode| NamespaceIdentity {
                device: [0, 4],
                inode,
            })
            .to_vec(),
        absent_cgroup_paths: Some(binding.cgroup_paths.clone()),
    };
    assert!(validate_cleanup_observer_record(&record, &binding, &op, &digest).is_ok());
    let mut leaked = record.clone();
    leaked.owned_references_absent = false;
    assert!(validate_cleanup_observer_record(&leaked, &binding, &op, &digest).is_err());
    let mut counted = record.clone();
    counted.owned_reference_count = 1;
    assert!(validate_cleanup_observer_record(&counted, &binding, &op, &digest).is_err());
    let mut incomplete = record.clone();
    incomplete.complete = false;
    assert!(validate_cleanup_observer_record(&incomplete, &binding, &op, &digest).is_err());
    let mut wrong_binding = record;
    wrong_binding.binding_sha256 = "c".repeat(64);
    assert!(validate_cleanup_observer_record(&wrong_binding, &binding, &op, &digest).is_err());
}
