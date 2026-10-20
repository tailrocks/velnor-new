use serde_json::{Value, json};

use super::canonical_line;

pub(crate) fn owned_cgroup_record(
    request: &velnor_resource_probe::worker_binding::OwnedCgroupRequest,
) -> Result<Vec<u8>, String> {
    request.validate().map_err(|error| error.to_string())?;
    let boot = "12345678-1234-1234-1234-123456789abc";
    let runner = owned_worker(
        "runner",
        &request.runner_container_id,
        request.runner_init_pid,
        request.runner_init_start_ticks,
        request.runner_helper_pid,
        request.runner_helper_start_ticks,
    );
    let dind = owned_worker(
        "dind",
        &request.dind_container_id,
        request.dind_init_pid,
        request.dind_init_start_ticks,
        request.dind_helper_pid,
        request.dind_helper_start_ticks,
    );
    let runner_witness = runner["init"].clone();
    let dind_witness = dind["init"].clone();
    let runner_helper = runner["helper"].clone();
    let dind_helper = dind["helper"].clone();
    let metadata = docker_root_attachment();
    let record = json!({
        "wire_mode": "owned_container_cgroup",
        "schema_version": velnor_resource_probe::worker_binding::OwnedCgroupObservation::SCHEMA_VERSION,
        "operation_id": request.operation_id,
        "guest_boot_id": boot,
        "engine_metadata": metadata,
        "docker_root_object": {
            "mount_id": 5,
            "device_major": 0,
            "device_minor": 41,
            "inode": 700,
            "filesystem_type_magic": 0x9123_683e_u64,
            "filesystem_id": 17
        },
        "daemon": process_witness(1410, 1, 1, "/system.slice/docker.service"),
        "containers": [runner, dind],
        "root_backings": [
            root_backing("runner", &request.runner_container_id, boot, runner_witness, runner_helper),
            root_backing("dind", &request.dind_container_id, boot, dind_witness, dind_helper)
        ]
    });
    let record: velnor_resource_probe::worker_binding::OwnedCgroupObservation =
        serde_json::from_value(record).map_err(|error| error.to_string())?;
    canonical_line(&record)
}

fn owned_worker(
    role: &str,
    container_id: &str,
    init_pid: u32,
    init_ticks: u64,
    helper_pid: u32,
    helper_ticks: u64,
) -> Value {
    let path = format!("/docker/{container_id}");
    let namespace = if role == "runner" { 4101 } else { 4201 };
    json!({
        "role": role,
        "container_id": container_id,
        "init": process_witness(init_pid, init_ticks, namespace, &path),
        "helper": process_witness(helper_pid, helper_ticks, namespace, &path),
        "hierarchy": [
            cgroup_node("/", true),
            cgroup_node(&path, false)
        ]
    })
}

fn process_witness(pid: u32, ticks: u64, namespace: u64, path: &str) -> Value {
    json!({
        "host_pid": pid,
        "start_ticks": ticks,
        "mount_namespace_id": namespace,
        "mount_namespace_device_major": 0,
        "mount_namespace_device_minor": 4,
        "namespace_pids": [pid, 1],
        "cgroup_path": path
    })
}

fn cgroup_node(path: &str, root: bool) -> Value {
    json!({
        "path": path,
        "root": root,
        "memory_max_bytes": if root { Value::Null } else { json!(4_294_967_296_u64) },
        "memory_current_bytes": 67_108_864_u64,
        "swap_max_bytes": 0_u64,
        "swap_current_bytes": 0_u64,
        "cpu_quota_micros": 100_000_u64,
        "cpu_period_micros": 100_000_u64,
        "cpuset_cpus": [0],
        "pids_max": 128_u64,
        "pids_current": 2_u64
    })
}

fn docker_root_attachment() -> Value {
    json!({
        "role": "engine_metadata",
        "target": "/velnor/docker-root",
        "read_only": true,
        "executable_mount": false,
        "mount_id": 5,
        "statx_mount_id": 5,
        "statx_inode": 700,
        "mount_root": "/_data",
        "mount_source": "/dev/vdb",
        "mount_device_major": 0,
        "mount_device_minor": 41,
        "statx_device_major": 0,
        "statx_device_minor": 41,
        "filesystem_type_magic": 0x9123_683e_u64,
        "filesystem_type_name": "btrfs",
        "filesystem_id": 17,
        "block_size": 4096_u64,
        "total_bytes": 42_949_672_960_u64,
        "free_bytes": 21_474_836_480_u64,
        "available_bytes": 20_401_094_656_u64,
        "inode_total": 0_u64,
        "inode_free": 0_u64,
        "inode_available": 0_u64,
        "inode_policy": "dynamic_btrfs"
    })
}

fn root_backing(role: &str, id: &str, boot: &str, init: Value, helper: Value) -> Value {
    let directories = [
        ("upper", format!("/var/lib/docker/overlay2/{id}/diff")),
        ("work", format!("/var/lib/docker/overlay2/{id}/work")),
        ("lower", format!("/var/lib/docker/overlay2/{id}/lower")),
    ]
    .into_iter()
    .map(|(kind, path)| {
        json!({
            "kind": kind,
            "path": path,
            "mount_root": "/_data",
            "mount_source": "/dev/vdb",
            "mount_filesystem_type": "btrfs",
            "mount_read_only": false,
            "mount_device_major": 0,
            "mount_device_minor": 41,
            "statx_mount_id": 5,
            "statx_device_major": 0,
            "statx_device_minor": 41,
            "filesystem_type_magic": 0x9123_683e_u64,
            "filesystem_id": 17,
            "block_size": 4096_u64,
            "total_bytes": 42_949_672_960_u64,
            "free_bytes": 21_474_836_480_u64,
            "available_bytes": 20_401_094_656_u64,
            "inode_total": 0_u64,
            "inode_free": 0_u64,
            "inode_available": 0_u64
        })
    })
    .collect::<Vec<_>>();
    json!({
        "role": role,
        "container_id": id,
        "guest_boot_id": boot,
        "init": init,
        "helper": helper,
        "mount_source": "overlay",
        "filesystem_type": "overlay",
        "mount_id": 500,
        "mount_device_major": 0,
        "mount_device_minor": 55,
        "mount_root": "/",
        "read_only": false,
        "upperdir": directories[0]["path"],
        "workdir": directories[1]["path"],
        "lowerdirs": [directories[2]["path"]],
        "directories": directories
    })
}
