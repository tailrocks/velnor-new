// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::TestResult;
use velnor_resource_probe::{
    InodePolicy,
    worker_binding::{
        AttachmentRole, CgroupNodeObservation, DockerRootObjectIdentity, OwnedCgroupObservation,
        OwnedContainerCgroupObservation, ProcessWitness, SelfLeafLimits, SelfStorageObservation,
        StorageAttachmentObservation, WorkerRole,
    },
};
use velnor_runner_host::{HostError, observed_cgroups};

#[test]
fn shared_cgroup_admits_unique_tmpfs_and_rejects_insufficient_capacity() -> TestResult<()> {
    let runner_id = &"c".repeat(64);
    let dind_id = &"d".repeat(64);
    let (runner_mem, dind_mem) = (12_431_917_056_u64, 13_505_658_880_u64);
    let (one_byte_short, exact_budget) = (16_206_790_655_u64, 16_206_790_656_u64);
    let runner_obs =
        sample_self_observation(WorkerRole::Runner, runner_id, "/docker/runner", runner_mem);
    let dind_obs = sample_self_observation(WorkerRole::Dind, dind_id, "/docker/dind", dind_mem);
    let node = |p, root, m, pids, q| cgroup_node(p, root, Some(m), Some(pids), Some(q));
    let r_leaf = node("/docker/runner", false, runner_mem, 512, 200_000);
    let d_leaf = node("/docker/dind", false, dind_mem, 512, 200_000);
    let check = |shared: &CgroupNodeObservation, runner_leaf: CgroupNodeObservation, dind_leaf| {
        let owned = sample_owned_observation(runner_id, dind_id, shared, runner_leaf, dind_leaf);
        observed_cgroups(&runner_obs, &dind_obs, &owned, runner_id, dind_id)
    };
    let ok_shared = node("/docker", true, exact_budget, 1_024, 400_000);
    let observed = check(&ok_shared, r_leaf.clone(), d_leaf.clone())?;
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].memory(), (Some(runner_mem), 1_048_576));
    assert_eq!(observed[1].memory(), (Some(dind_mem), 1_048_576));

    let low_mem = node("/docker", true, one_byte_short, 1_024, 400_000);
    assert!(matches!(
        check(&low_mem, r_leaf.clone(), d_leaf.clone()),
        Err(HostError::Identity)
    ));

    let mut bad_dind_obs = dind_obs.clone();
    bad_dind_obs.attachments[1].filesystem_id += 1;
    let ok_owned = sample_owned_observation(
        runner_id,
        dind_id,
        &ok_shared,
        r_leaf.clone(),
        d_leaf.clone(),
    );
    let bad_att = observed_cgroups(&runner_obs, &bad_dind_obs, &ok_owned, runner_id, dind_id);
    assert!(matches!(bad_att, Err(HostError::Identity)));

    let mut different_period = node("/docker", true, exact_budget, 1_024, 800_000);
    different_period.cpu_period_micros = Some(200_000);
    assert!(check(&different_period, r_leaf.clone(), d_leaf.clone()).is_ok());
    different_period.cpu_quota_micros = Some(400_000);
    assert!(matches!(
        check(&different_period, r_leaf.clone(), d_leaf.clone()),
        Err(HostError::Identity)
    ));

    let low_cpu = node("/docker", true, exact_budget, 1_024, 200_000);
    assert!(matches!(
        check(&low_cpu, r_leaf, d_leaf.clone()),
        Err(HostError::Identity)
    ));

    let capped = node("/docker/runner", false, runner_mem / 2, 512, 200_000);
    assert!(matches!(
        check(&ok_shared, capped, d_leaf),
        Err(HostError::Identity)
    ));
    Ok(())
}

fn cgroup_node(
    path: &str,
    root: bool,
    memory_max_bytes: Option<u64>,
    pids_max: Option<u64>,
    cpu_quota_micros: Option<u64>,
) -> CgroupNodeObservation {
    CgroupNodeObservation {
        path: path.to_owned(),
        root,
        memory_max_bytes,
        memory_current_bytes: Some(1_048_576),
        swap_max_bytes: memory_max_bytes,
        swap_current_bytes: Some(0),
        cpu_quota_micros,
        cpu_period_micros: Some(100_000),
        cpuset_cpus: Some(vec![0, 1]),
        pids_max,
        pids_current: Some(8),
    }
}

fn attachment_obs(
    role: AttachmentRole,
    target: &str,
    minor: u32,
    total_bytes: u64,
    btrfs: bool,
) -> StorageAttachmentObservation {
    let (fs_magic, fs_name, policy) = if btrfs {
        (0x9123_683e, "btrfs", InodePolicy::DynamicBtrfs)
    } else {
        (0x0102_1994, "tmpfs", InodePolicy::Finite)
    };
    StorageAttachmentObservation {
        role,
        target: target.to_owned(),
        read_only: btrfs,
        executable_mount: true,
        mount_id: u64::from(minor),
        statx_mount_id: u64::from(minor),
        statx_inode: u64::from(minor),
        mount_root: "/".to_owned(),
        mount_source: if btrfs { "/dev/vda1" } else { "tmpfs" }.to_owned(),
        mount_device_major: 0,
        mount_device_minor: minor,
        statx_device_major: 0,
        statx_device_minor: if btrfs { 41 } else { minor },
        filesystem_type_magic: fs_magic,
        filesystem_type_name: fs_name.to_owned(),
        filesystem_id: if btrfs { 77 } else { u64::from(minor) },
        block_size: 4096,
        total_bytes,
        free_bytes: total_bytes,
        available_bytes: total_bytes,
        inode_total: u64::from(!btrfs) * 1024,
        inode_free: u64::from(!btrfs) * 1024,
        inode_available: u64::from(!btrfs) * 1024,
        inode_policy: policy,
    }
}

fn sample_self_observation(
    role: WorkerRole,
    container_id: &str,
    cgroup_path: &str,
    memory_max_bytes: u64,
) -> SelfStorageObservation {
    let shared = [
        (
            AttachmentRole::RunnerHome,
            "/home/runner",
            51,
            1_073_741_824,
        ),
        (
            AttachmentRole::RunnerWork,
            "/home/runner/_work",
            52,
            8_589_934_592,
        ),
        (AttachmentRole::SharedRun, "/run", 53, 67_108_864),
    ];
    SelfStorageObservation {
        wire_mode: "self_storage".to_owned(),
        schema_version: 1,
        operation_id: "b".repeat(32),
        container_id: container_id.to_owned(),
        role,
        helper_protocol_version: 3,
        helper_source_revision: "1".repeat(40),
        guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        process_pid: 20,
        process_start_ticks: 100,
        init_start_ticks: 90,
        mount_namespace_id: 4002,
        mount_namespace_device_major: 0,
        mount_namespace_device_minor: 4,
        cgroup_namespace_id: 5002,
        cgroup_path_in_self_namespace: "/".to_owned(),
        self_leaf_limits: SelfLeafLimits {
            cgroup_path: cgroup_path.to_owned(),
            memory_max_bytes: Some(memory_max_bytes),
            memory_current_bytes: 1_048_576,
            swap_max_bytes: Some(memory_max_bytes),
            swap_current_bytes: 0,
            cpu_quota_micros: Some(200_000),
            cpu_period_micros: 100_000,
            cpuset_cpus: vec![0, 1],
            pids_max: Some(512),
            pids_current: 8,
        },
        attachments: shared
            .into_iter()
            .map(|(r, t, m, b)| attachment_obs(r, t, m, b, false))
            .collect(),
    }
}

fn sample_owned_observation(
    runner_id: &str,
    dind_id: &str,
    shared: &CgroupNodeObservation,
    runner_leaf: CgroupNodeObservation,
    dind_leaf: CgroupNodeObservation,
) -> OwnedCgroupObservation {
    let witness = |pid, cgroup_path: &str| ProcessWitness {
        host_pid: pid,
        start_ticks: 100,
        mount_namespace_id: 4000 + u64::from(pid),
        mount_namespace_device_major: 0,
        mount_namespace_device_minor: 4,
        namespace_pids: vec![1],
        cgroup_path: cgroup_path.to_owned(),
    };
    let cont =
        |role, cid: &str, pid, leaf: CgroupNodeObservation| OwnedContainerCgroupObservation {
            role,
            container_id: cid.to_owned(),
            init: witness(pid, &leaf.path),
            helper: witness(pid + 1, &leaf.path),
            hierarchy: vec![shared.clone(), leaf],
        };
    OwnedCgroupObservation {
        wire_mode: "owned_cgroups".to_owned(),
        schema_version: OwnedCgroupObservation::SCHEMA_VERSION,
        operation_id: "b".repeat(32),
        guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        engine_metadata: attachment_obs(
            AttachmentRole::EngineMetadata,
            "/velnor/docker-root",
            38,
            68_719_476_736,
            true,
        ),
        docker_root_object: DockerRootObjectIdentity {
            mount_id: 90,
            device_major: 0,
            device_minor: 38,
            inode: 1,
            filesystem_type_magic: 0x9123_683e,
            filesystem_id: 77,
        },
        daemon: witness(10, "/docker"),
        containers: vec![
            cont(WorkerRole::Runner, runner_id, 20, runner_leaf),
            cont(WorkerRole::Dind, dind_id, 30, dind_leaf),
        ],
        root_backings: Vec::new(),
    }
}
