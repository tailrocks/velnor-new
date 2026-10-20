use serde_json::{Value, json};
use velnor_resource_probe::worker_binding::{
    AttachmentRole, SelfStorageObservation, SelfStorageRequest, WorkerRole,
};

use super::canonical_line;

pub(crate) fn self_storage_record(
    request: &SelfStorageRequest,
    source_revision: &str,
    metadata_inodes: [u64; 3],
) -> Result<Vec<u8>, String> {
    let attachments = attachment_facts(request.role, metadata_inodes)?;
    let role = match request.role {
        WorkerRole::Runner => "runner",
        WorkerRole::Dind => "dind",
    };
    let record = json!({
        "wire_mode": "self_storage",
        "schema_version": 1,
        "operation_id": request.operation_id,
        "container_id": request.container_id,
        "role": role,
        "helper_protocol_version": 3,
        "helper_source_revision": source_revision,
        "guest_boot_id": "12345678-1234-1234-1234-123456789abc",
        "process_pid": 301,
        "process_start_ticks": 9001,
        "init_start_ticks": 8001,
        "mount_namespace_id": 4101,
        "mount_namespace_device_major": 0,
        "mount_namespace_device_minor": 4,
        "cgroup_namespace_id": 4102,
        "cgroup_path_in_self_namespace": format!("/worker/{role}/helper"),
        "self_leaf_limits": {
            "cgroup_path": format!("/worker/{role}/helper"),
            "memory_max_bytes": 4294967296_u64,
            "memory_current_bytes": 67108864_u64,
            "swap_max_bytes": 0_u64,
            "swap_current_bytes": 0_u64,
            "cpu_quota_micros": 100000_u64,
            "cpu_period_micros": 100000_u64,
            "cpuset_cpus": [0],
            "pids_max": 128_u64,
            "pids_current": 2_u64
        },
        "attachments": attachments
    });
    let record: SelfStorageObservation =
        serde_json::from_value(record).map_err(|error| error.to_string())?;
    canonical_line(&record)
}

type AttachmentSpec = (AttachmentRole, &'static str, bool, &'static str, bool);

fn attachment_facts(role: WorkerRole, metadata_inodes: [u64; 3]) -> Result<Vec<Value>, String> {
    let specs = match role {
        WorkerRole::Runner => runner_attachment_specs(),
        WorkerRole::Dind => dind_attachment_specs(),
    };
    specs
        .into_iter()
        .enumerate()
        .map(|(index, spec)| attachment_record(index, spec, metadata_inodes))
        .collect()
}

fn runner_attachment_specs() -> Vec<AttachmentSpec> {
    vec![
        (AttachmentRole::RunnerRoot, "/", true, "docker-root", false),
        (
            AttachmentRole::RunnerHome,
            "/home/runner",
            false,
            "runner-home",
            true,
        ),
        (
            AttachmentRole::RunnerWork,
            "/home/runner/_work",
            false,
            "runner-work",
            true,
        ),
        (AttachmentRole::RunnerTmp, "/tmp", false, "runner-tmp", true),
        (
            AttachmentRole::RunnerShm,
            "/dev/shm",
            false,
            "runner-shm",
            true,
        ),
        (AttachmentRole::SharedRun, "/run", false, "shared-run", true),
        (
            AttachmentRole::JobFacts,
            "/opt/velnor/job-facts",
            true,
            "metadata",
            false,
        ),
        (
            AttachmentRole::RuntimeIdentity,
            "/.velnor-runtime-identity",
            true,
            "metadata",
            false,
        ),
        (
            AttachmentRole::SourceProofControl,
            "/opt/velnor/source-proof/control",
            true,
            "metadata",
            false,
        ),
        (
            AttachmentRole::SourceProofInbox,
            "/opt/velnor/source-proof/inbox",
            false,
            "runner-home",
            true,
        ),
    ]
}

fn dind_attachment_specs() -> Vec<AttachmentSpec> {
    vec![
        (AttachmentRole::DindRoot, "/", true, "docker-root", false),
        (
            AttachmentRole::RunnerHome,
            "/home/runner",
            false,
            "runner-home",
            true,
        ),
        (
            AttachmentRole::RunnerWork,
            "/home/runner/_work",
            false,
            "runner-work",
            true,
        ),
        (AttachmentRole::SharedRun, "/run", false, "shared-run", true),
        (
            AttachmentRole::DindData,
            "/var/lib/docker",
            false,
            "dind-data",
            true,
        ),
        (AttachmentRole::DindTmp, "/tmp", false, "dind-tmp", true),
        (AttachmentRole::DindShm, "/dev/shm", false, "dind-shm", true),
        (
            AttachmentRole::JobFacts,
            "/opt/velnor/job-facts",
            true,
            "metadata",
            false,
        ),
        (
            AttachmentRole::RuntimeIdentity,
            "/.velnor-runtime-identity",
            true,
            "metadata",
            false,
        ),
    ]
}

fn attachment_record(
    index: usize,
    spec: AttachmentSpec,
    metadata_inodes: [u64; 3],
) -> Result<Value, String> {
    let (role, target, read_only, source, tmpfs) = spec;
    let (tmpfs_bytes, tmpfs_inodes) = if tmpfs {
        tmpfs_capacity(source)?
    } else {
        (0, 0)
    };
    let subpath = match target {
        "/opt/velnor/job-facts" => Some("job-facts"),
        "/.velnor-runtime-identity" => Some("runtime-identity"),
        "/opt/velnor/source-proof/control" => Some("source-proof/control"),
        "/opt/velnor/source-proof/inbox" => Some("source-proof/inbox"),
        _ => None,
    };
    let device_minor = attachment_device_minor(tmpfs, source, role, false);
    let statx_device_minor = attachment_device_minor(tmpfs, source, role, true);
    Ok(json!({
        "role": serde_json::to_value(role).unwrap_or(Value::Null),
        "target": target,
        "read_only": read_only,
        "executable_mount": false,
        "mount_id": 500 + index as u64,
        "statx_mount_id": 500 + index as u64,
        "statx_inode": attachment_inode(target, source, index, metadata_inodes),
        "mount_root": subpath.map_or_else(|| "/".to_owned(), |path| format!("/fixture/{path}")),
        "mount_source": attachment_source(tmpfs, source),
        "mount_device_major": 0,
        "mount_device_minor": device_minor,
        "statx_device_major": 0,
        "statx_device_minor": statx_device_minor,
        "filesystem_type_magic": if tmpfs { 0x01021994_u64 } else { 0x9123683e_u64 },
        "filesystem_type_name": if tmpfs { "tmpfs" } else { "btrfs" },
        "filesystem_id": fixture_filesystem_id(source, index),
        "block_size": 4096_u64,
        "total_bytes": fixture_total_bytes(tmpfs, source, tmpfs_bytes),
        "free_bytes": fixture_free_bytes(tmpfs, source, tmpfs_bytes),
        "available_bytes": fixture_free_bytes(tmpfs, source, tmpfs_bytes),
        "inode_total": if tmpfs { tmpfs_inodes } else { 1000000_u64 },
        "inode_free": fixture_free_inodes(tmpfs, source, tmpfs_inodes),
        "inode_available": fixture_free_inodes(tmpfs, source, tmpfs_inodes),
        "inode_policy": "finite"
    }))
}

fn attachment_device_minor(tmpfs: bool, source: &str, role: AttachmentRole, statx: bool) -> u32 {
    if tmpfs {
        fixture_tmpfs_device_minor(role)
    } else if source == "metadata" {
        48
    } else if source == "docker-root" {
        41
    } else if statx {
        57
    } else {
        56
    }
}

fn attachment_inode(target: &str, source: &str, index: usize, metadata_inodes: [u64; 3]) -> u64 {
    match target {
        "/.velnor-runtime-identity" => metadata_inodes[0],
        "/opt/velnor/job-facts" => metadata_inodes[1],
        "/opt/velnor/source-proof/control" => metadata_inodes[2],
        _ if source == "shared-run" => 650,
        _ => 600 + index as u64,
    }
}

fn attachment_source(tmpfs: bool, source: &str) -> String {
    if tmpfs {
        "tmpfs".to_owned()
    } else if source == "docker-root" {
        "/dev/vdb".to_owned()
    } else {
        format!("/dev/{source}")
    }
}

fn fixture_filesystem_id(source: &str, index: usize) -> u64 {
    match source {
        "runner-home" => 11,
        "runner-work" => 12,
        "shared-run" => 13,
        "metadata" => 14,
        "docker-root" => 17,
        _ => 20 + index as u64,
    }
}

fn fixture_total_bytes(tmpfs: bool, source: &str, tmpfs_bytes: u64) -> u64 {
    if tmpfs {
        tmpfs_bytes
    } else if source == "runner-work" {
        4294967296
    } else {
        2147483648
    }
}

fn fixture_free_bytes(tmpfs: bool, source: &str, tmpfs_bytes: u64) -> u64 {
    if tmpfs && source == "runner-work" {
        tmpfs_bytes - 294967296
    } else if tmpfs {
        tmpfs_bytes
    } else if source == "runner-work" {
        4000000000
    } else {
        2147483648
    }
}

fn fixture_free_inodes(tmpfs: bool, source: &str, tmpfs_inodes: u64) -> u64 {
    if tmpfs && source == "runner-work" {
        tmpfs_inodes * 3 / 4
    } else if tmpfs {
        tmpfs_inodes
    } else {
        1000000
    }
}

fn tmpfs_capacity(source: &str) -> Result<(u64, u64), String> {
    let role = match source {
        "runner-home" => crate::journal::StorageRole::RunnerHome,
        "runner-work" => crate::journal::StorageRole::RunnerWork,
        "runner-tmp" => crate::journal::StorageRole::RunnerTmp,
        "runner-shm" => crate::journal::StorageRole::RunnerShm,
        "shared-run" => crate::journal::StorageRole::SharedRun,
        "dind-data" => crate::journal::StorageRole::DindData,
        "dind-tmp" => crate::journal::StorageRole::DindTmp,
        "dind-shm" => crate::journal::StorageRole::DindShm,
        _ => return Err("unknown tmpfs fixture source".to_owned()),
    };
    let profile = crate::journal::fixture_provisioning_profile();
    let limit = profile
        .role_limit(role)
        .map_err(|_| "tmpfs role has no fixture capacity".to_owned())?;
    Ok((limit.max_bytes(), limit.max_inodes()))
}

fn fixture_tmpfs_device_minor(role: AttachmentRole) -> u32 {
    match role {
        AttachmentRole::RunnerHome => 51,
        AttachmentRole::SourceProofInbox => 51,
        AttachmentRole::RunnerWork => 52,
        AttachmentRole::RunnerTmp => 53,
        AttachmentRole::RunnerShm => 54,
        AttachmentRole::SharedRun => 55,
        AttachmentRole::DindData => 56,
        AttachmentRole::DindTmp => 57,
        AttachmentRole::DindShm => 58,
        _ => 55,
    }
}
