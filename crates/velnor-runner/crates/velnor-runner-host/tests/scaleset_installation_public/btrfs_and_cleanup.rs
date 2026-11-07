// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_resource_probe::{
    cleanup_observer_wire::{
        CleanupDockerRootIdentity, CleanupObserverBinding, CleanupVolume, ProcessIdentity,
        docker_root_matches, validate_binding,
    },
    execution_channel_wire::ExecutionChannelObservation,
};
use velnor_runner_host::{HostError, ProofChannelBindingInput, VerifiedProofChannelBinding};

use crate::{TestResult, directory_and_mount};

fn sample_cleanup_binding(
    filesystem_type_magic: u64,
    mount_device: [u32; 2],
    statx_device: [u32; 2],
) -> CleanupObserverBinding {
    let proc_id = |pid, ns_ino| ProcessIdentity {
        pid,
        start_ticks: 100,
        namespace_inode: ns_ino,
        namespace_device: [0, 4],
    };
    let vol = |role: &str, inode| CleanupVolume {
        role: role.to_owned(),
        mountpoint: format!("/var/lib/docker/volumes/{role}/_data"),
        mount_device,
        statx_device,
        inode,
    };
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
        cgroup_paths: [
            "/docker/1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            "/docker/2222222222222222222222222222222222222222222222222222222222222222".to_owned(),
        ],
        volume_mountpoints: vec![
            vol("shared-run", 101),
            vol("runner-home", 102),
            vol("runner-work", 103),
            vol("dind-data", 104),
            vol("engine-metadata", 105),
        ],
        owned_tmpfs_devices: [
            [0, 51],
            [0, 52],
            [0, 53],
            [0, 54],
            [0, 55],
            [0, 56],
            [0, 57],
            [0, 58],
        ],
    }
}

fn build_inbox_binding(
    fs: (&str, u64),
    mount_source: &str,
    statx_minor: u32,
) -> Result<VerifiedProofChannelBinding, HostError> {
    let (control_dir, control_mount) = directory_and_mount(
        "/opt/velnor/source-proof/control",
        "source-proof/control",
        101,
        11,
        true,
        true,
        ("btrfs", 0x9123_683e),
    );
    let (mut inbox_dir, mut inbox_mount) = directory_and_mount(
        "/opt/velnor/source-proof/inbox",
        "source-proof/inbox",
        202,
        22,
        false,
        true,
        fs,
    );
    inbox_dir.mount_source = mount_source.to_owned();
    inbox_dir.mount_device_major = 0;
    inbox_dir.mount_device_minor = 38;
    inbox_dir.statx_device_major = 0;
    inbox_dir.statx_device_minor = statx_minor;
    inbox_mount.device_major = 0;
    inbox_mount.device_minor = 38;
    inbox_mount.statx_device_major = 0;
    inbox_mount.statx_device_minor = statx_minor;
    let (job_facts_dir, job_facts_mount) = directory_and_mount(
        "/opt/velnor/job-facts",
        "job-facts",
        303,
        33,
        true,
        true,
        ("btrfs", 0x9123_683e),
    );
    VerifiedProofChannelBinding::from_measured(ProofChannelBindingInput {
        reservation_id: 1,
        operation_id: 2,
        connection_id: "conn-1".to_owned(),
        scale_set_id: 3,
        witness_request_id: 4,
        engine_id: "engine-1".to_owned(),
        docker_root_digest: "b".repeat(64),
        runner_id: 5,
        runner_name: "velnor-runner-1".to_owned(),
        runner_container_id: "c".repeat(64),
        dind_container_id: "d".repeat(64),
        instance_binding_digest: "e".repeat(64),
        metadata_volume_name: "velnor-meta-1".to_owned(),
        metadata_volume_id: "velnor-meta-id-1".to_owned(),
        control: control_mount,
        inbox: inbox_mount,
        job_facts: job_facts_mount,
        observation: ExecutionChannelObservation {
            schema_version: 3,
            operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
            guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
            mount_namespace_id: 4_026_531_840,
            control: control_dir,
            inbox: inbox_dir,
            job_facts: job_facts_dir,
        },
        dind_attachment_absent: true,
    })
}

#[test]
fn public_btrfs_subpath_inbox_and_cleanup_docker_root_device_contracts() -> TestResult<()> {
    let btrfs_inbox = build_inbox_binding(("btrfs", 0x9123_683e), "/dev/vdb1", 41)?;
    assert_eq!(btrfs_inbox.channel_digest().len(), 64);
    assert!(matches!(
        build_inbox_binding(("ext4", 0xef53), "/dev/vdb1", 41),
        Err(HostError::Identity)
    ));
    assert!(matches!(
        build_inbox_binding(("btrfs", 0x9123_683e), "", 41),
        Err(HostError::Identity)
    ));

    let btrfs_cleanup = sample_cleanup_binding(0x9123_683e, [0, 38], [0, 41]);
    assert_eq!(validate_binding(&btrfs_cleanup), Ok(()));
    assert!(docker_root_matches(
        &btrfs_cleanup.docker_root,
        &btrfs_cleanup.docker_root
    ));

    let ext4_split_cleanup = sample_cleanup_binding(0xef53, [0, 38], [0, 41]);
    assert_eq!(
        validate_binding(&ext4_split_cleanup),
        Err("cleanup_docker_root_identity")
    );
    let mut mismatched_statx = btrfs_cleanup.docker_root.clone();
    mismatched_statx.statx_device = [0, 42];
    assert!(!docker_root_matches(
        &btrfs_cleanup.docker_root,
        &mismatched_statx
    ));
    Ok(())
}
