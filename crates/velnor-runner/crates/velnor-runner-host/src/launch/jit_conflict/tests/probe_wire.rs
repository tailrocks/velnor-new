// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::TestResult;
use serde_json::{Value, json};
use velnor_resource_probe::cleanup_observer_wire::{
    CleanupObserverRecord, CleanupObserverRequest, NamespaceIdentity,
};
use velnor_runner_qualification::probe_record::{CpuAncestorLimit, InodePolicy, ProbeRecord};

fn image_config() -> Value {
    json!({"User": "65532:65532", "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"], "WorkingDir": "/", "Entrypoint": ["/velnor/resource-probe"], "Labels": {"org.opencontainers.image.revision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}})
}

pub(super) fn image(id: &str) -> Value {
    json!({"Id": id, "Os": "linux", "Architecture": "amd64", "Config": image_config()})
}

pub(super) fn inspect(
    id: &str,
    name: &str,
    labels: &Value,
    running: bool,
    exited: bool,
    requested: Option<&Value>,
) -> TestResult<Value> {
    let mut config = image_config();
    let mut host = Value::Null;
    let mut mounts = Vec::new();
    if let Some(requested) = requested {
        for (key, value) in requested.as_object().ok_or("create object")? {
            if key == "HostConfig" {
                host = value.clone();
            } else if !value.is_null() && key != "NetworkingConfig" {
                if key == "Labels" {
                    for (label, value) in value.as_object().ok_or("labels")? {
                        config["Labels"][label] = value.clone();
                    }
                } else {
                    config[key] = value.clone();
                }
            }
        }
        for mount in host["Mounts"].as_array().ok_or("probe mounts")? {
            mounts.push(json!({"Type": "bind", "Source": mount["Source"], "Destination": mount["Target"], "RW": false}));
        }
    } else {
        config = json!({"Labels": labels});
    }
    let status = if running {
        "running"
    } else if exited {
        "exited"
    } else {
        "created"
    };
    Ok(
        json!({"Id": id, "Name": format!("/{name}"), "Image": config["Image"], "RestartCount": 0, "Path": "/velnor/resource-probe", "Config": config, "HostConfig": host, "Mounts": mounts, "State": {"Running": running, "Status": status, "Pid": 1234, "ExitCode": 0, "OOMKilled": false}}),
    )
}

pub(super) fn observer(input: &[u8]) -> TestResult<Vec<u8>> {
    let mut lines = input.split(|byte| *byte == b'\n');
    let line = lines.next().ok_or("observer request")?;
    let request: CleanupObserverRequest = serde_json::from_slice(line)?;
    let acknowledgement: velnor_resource_probe::worker_binding::AcknowledgeRequest =
        serde_json::from_slice(lines.next().ok_or("observer acknowledgement")?)?;
    if acknowledgement.operation_id != request.operation_id {
        return Err("observer acknowledgement identity".into());
    }
    let binding = request.binding;
    let record = CleanupObserverRecord {
        schema_version: 1,
        operation_id: request.operation_id,
        binding_sha256: request.binding_sha256,
        guest_boot_id: Some(binding.expected_guest_boot_id),
        docker_root: Some(binding.docker_root),
        complete: true,
        owned_references_absent: true,
        issues: Vec::new(),
        task_count: 4,
        descriptor_count: 4,
        namespace_count: 1,
        owned_reference_count: 0,
        absent_volume_mountpoints: binding
            .volume_mountpoints
            .into_iter()
            .map(|volume| volume.mountpoint)
            .collect(),
        absent_tmpfs_devices: binding.owned_tmpfs_devices.to_vec(),
        absent_mount_namespaces: [binding.runner, binding.dind]
            .into_iter()
            .map(|process| NamespaceIdentity {
                device: process.namespace_device,
                inode: process.namespace_inode,
            })
            .collect(),
        absent_cgroup_paths: Some(binding.cgroup_paths),
    };
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn frame(output: &[u8]) -> TestResult<Vec<u8>> {
    let mut bytes = vec![1, 0, 0, 0];
    bytes.extend_from_slice(&u32::try_from(output.len())?.to_be_bytes());
    bytes.extend_from_slice(output);
    Ok(bytes)
}

pub(super) fn capacity() -> TestResult<Vec<u8>> {
    let record = ProbeRecord {
        schema_version: 5,
        docker_root_mount_target: "/velnor/docker-root".to_owned(),
        docker_root_mount_source: "/dev/vda1".to_owned(),
        docker_root_mount_root: "/docker".to_owned(),
        docker_root_mount_id: 90,
        docker_root_device_major: 0,
        docker_root_device_minor: 38,
        docker_root_statx_device_major: 0,
        docker_root_statx_device_minor: 38,
        docker_root_fs_type_magic: 0x9123_683e,
        docker_root_fs_type_name: "btrfs".to_owned(),
        docker_root_filesystem_id: 77,
        guest_boot_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        docker_root_free_bytes: 90_000_000_000,
        docker_root_total_bytes: 100_000_000_000,
        docker_root_available_bytes: 90_000_000_000,
        docker_root_inode_total: 0,
        docker_root_inode_free: 0,
        docker_root_inode_available: 0,
        docker_root_inode_policy: InodePolicy::DynamicBtrfs,
        memory_available_bytes: 4_000_000_000,
        load_milli: 0,
        memory_psi_some_avg10_bps: Some(0),
        probe_cgroup_path: "/probe".to_owned(),
        probe_cgroup_memory_max_bytes: Some(67_108_864),
        probe_cgroup_memory_current_bytes: 4_194_304,
        probe_cgroup_swap_max_bytes: Some(0),
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: Some(10_000),
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 4,
        cpu_ancestor_limits: vec![CpuAncestorLimit {
            path: "/".to_owned(),
            cpuset_cpus_count: 4,
            cpu_quota_micros: None,
            cpu_period_micros: 100_000,
        }],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: Some(4),
    };
    let mut bytes = serde_json::to_vec(&record)?;
    bytes.push(b'\n');
    Ok(bytes)
}
