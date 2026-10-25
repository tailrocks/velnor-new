use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::resource_probe_record::{CpuAncestorLimit, InodePolicy, MAX_OUTPUT_BYTES, ProbeRecord};

use super::validate;

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Result<Self, HostError> {
        let path = std::env::temp_dir().join(format!(
            "velnor-resource-probe-schema5-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).map_err(|_| HostError::Path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("temporary fixture cleanup succeeds");
    }
}

fn fixture_record() -> ProbeRecord {
    ProbeRecord {
        schema_version: 5,
        docker_root_mount_target: "/velnor/docker-root".to_owned(),
        docker_root_mount_source: "/dev/vdb".to_owned(),
        docker_root_mount_root: "/_data".to_owned(),
        docker_root_mount_id: 5,
        docker_root_device_major: 0,
        docker_root_device_minor: 41,
        docker_root_statx_device_major: 0,
        docker_root_statx_device_minor: 41,
        docker_root_fs_type_magic: 0x9123_683e,
        docker_root_fs_type_name: "btrfs".to_owned(),
        docker_root_filesystem_id: 17,
        guest_boot_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
        docker_root_free_bytes: 20 * 1024 * 1024 * 1024,
        docker_root_total_bytes: 40 * 1024 * 1024 * 1024,
        docker_root_available_bytes: 19 * 1024 * 1024 * 1024,
        docker_root_inode_total: 0,
        docker_root_inode_free: 0,
        docker_root_inode_available: 0,
        docker_root_inode_policy: InodePolicy::DynamicBtrfs,
        memory_available_bytes: 8 * 1024 * 1024 * 1024,
        load_milli: 125,
        memory_psi_some_avg10_bps: None,
        probe_cgroup_path: "/system.slice/velnor-resource-probe.service".to_owned(),
        probe_cgroup_memory_max_bytes: Some(512 * 1024 * 1024),
        probe_cgroup_memory_current_bytes: 64 * 1024 * 1024,
        probe_cgroup_swap_max_bytes: Some(0),
        probe_cgroup_swap_current_bytes: 0,
        probe_cgroup_cpu_quota_micros: Some(10_000),
        probe_cgroup_cpu_period_micros: 100_000,
        probe_cgroup_cpuset_cpus_count: 8,
        cpu_ancestor_limits: vec![
            CpuAncestorLimit {
                path: "/".to_owned(),
                cpuset_cpus_count: 8,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
            CpuAncestorLimit {
                path: "/system.slice".to_owned(),
                cpuset_cpus_count: 8,
                cpu_quota_micros: None,
                cpu_period_micros: 100_000,
            },
        ],
        probe_host_pid: 1234,
        probe_namespace_pid: 1234,
        probe_cgroup_pids_max: Some(64),
    }
}

fn record_bytes(record: &ProbeRecord) -> Result<Vec<u8>, HostError> {
    let mut bytes = serde_json::to_vec(record).map_err(|_| HostError::Identity)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_bytes(bytes: &[u8]) -> Result<(), HostError> {
    let directory = TempDirectory::new()?;
    let root = directory.path().join("root");
    std::fs::create_dir(&root).map_err(|_| HostError::Path)?;
    let output = directory.path().join("output.json");
    std::fs::write(&output, bytes).map_err(|_| HostError::Path)?;
    validate(&output, &root)
}

fn pad_to_limit(record: &mut ProbeRecord) -> Result<Vec<u8>, HostError> {
    let bytes = record_bytes(record)?;
    let padding = MAX_OUTPUT_BYTES
        .checked_sub(bytes.len())
        .ok_or(HostError::Frame)?;
    let source_room = 1024usize.saturating_sub(record.docker_root_mount_source.len());
    let source_padding = padding.min(source_room);
    record
        .docker_root_mount_source
        .push_str(&"x".repeat(source_padding));
    let mount_root_padding = padding - source_padding;
    if mount_root_padding > 1024usize.saturating_sub(record.docker_root_mount_root.len()) {
        return Err(HostError::Frame);
    }
    record
        .docker_root_mount_root
        .push_str(&"x".repeat(mount_root_padding));
    record_bytes(record)
}

#[test]
fn validates_schema5_at_the_exact_wire_size_limit() -> Result<(), HostError> {
    let mut record = fixture_record();
    let bytes = pad_to_limit(&mut record)?;
    assert_eq!(bytes.len(), MAX_OUTPUT_BYTES);
    validate_bytes(&bytes)?;

    record.docker_root_mount_root.push('x');
    let oversized = record_bytes(&record)?;
    assert_eq!(oversized.len(), MAX_OUTPUT_BYTES + 1);
    assert_eq!(validate_bytes(&oversized), Err(HostError::Frame));
    Ok(())
}

#[test]
fn schema5_entry_rejects_schema1_without_fallback() -> Result<(), HostError> {
    let schema1 = b"{\"schema_version\":1,\"docker_root_free_bytes\":4,\"docker_root_total_bytes\":8,\"memory_available_bytes\":2,\"load_milli\":3,\"memory_psi_some_avg10_bps\":10000}\n";
    assert!(ProbeRecord::parse_schema5(schema1).is_none());
    assert_eq!(validate_bytes(schema1), Err(HostError::Identity));
    Ok(())
}

#[test]
fn rejects_noncanonical_schema5_framing_and_field_sets() -> Result<(), HostError> {
    let valid = record_bytes(&fixture_record())?;
    assert!(ProbeRecord::parse_schema5(&valid[..valid.len() - 1]).is_none());

    let mut two_newlines = valid.clone();
    two_newlines.push(b'\n');
    assert!(ProbeRecord::parse_schema5(&two_newlines).is_none());

    let mut reordered = serde_json::to_vec(
        &serde_json::to_value(fixture_record()).map_err(|_| HostError::Identity)?,
    )
    .map_err(|_| HostError::Identity)?;
    reordered.push(b'\n');
    assert!(ProbeRecord::parse_schema5(&reordered).is_none());

    let duplicate = valid
        .windows(b"\"schema_version\":5".len())
        .position(|part| part == b"\"schema_version\":5")
        .map(|position| {
            let mut bytes = valid.clone();
            bytes.splice(position..position, b"\"schema_version\":5,".iter().copied());
            bytes
        })
        .ok_or(HostError::Identity)?;
    assert!(ProbeRecord::parse_schema5(&duplicate).is_none());

    let unknown = valid
        .strip_suffix(b"\n")
        .map(|body| [b"{\"unknown\":0,".as_slice(), &body[1..], b"\n"].concat())
        .ok_or(HostError::Identity)?;
    assert!(ProbeRecord::parse_schema5(&unknown).is_none());
    Ok(())
}

#[test]
fn rejects_invalid_mount_device_inode_and_filesystem_witnesses() -> Result<(), HostError> {
    let mut wrong_target = fixture_record();
    wrong_target.docker_root_mount_target = "/tmp".to_owned();
    assert!(ProbeRecord::parse_schema5(&record_bytes(&wrong_target)?).is_none());

    let mut missing_source = fixture_record();
    missing_source.docker_root_mount_source.clear();
    assert!(ProbeRecord::parse_schema5(&record_bytes(&missing_source)?).is_none());

    let mut wrong_device = fixture_record();
    wrong_device.docker_root_fs_type_magic = 0xEF53;
    wrong_device.docker_root_fs_type_name = "ext4".to_owned();
    wrong_device.docker_root_inode_policy = InodePolicy::Finite;
    wrong_device.docker_root_inode_total = 100;
    wrong_device.docker_root_inode_free = 90;
    wrong_device.docker_root_inode_available = 80;
    wrong_device.docker_root_statx_device_minor += 1;
    assert!(ProbeRecord::parse_schema5(&record_bytes(&wrong_device)?).is_none());

    let mut wrong_inodes = fixture_record();
    wrong_inodes.docker_root_inode_policy = InodePolicy::Finite;
    assert!(ProbeRecord::parse_schema5(&record_bytes(&wrong_inodes)?).is_none());
    Ok(())
}

#[test]
fn rejects_bad_cgroup_capacity_cpu_ancestry_and_pid_binding() -> Result<(), HostError> {
    let mut over_memory = fixture_record();
    over_memory.probe_cgroup_memory_current_bytes = 512 * 1024 * 1024 + 1;
    assert!(ProbeRecord::parse_schema5(&record_bytes(&over_memory)?).is_none());

    let mut reordered_ancestors = fixture_record();
    reordered_ancestors.cpu_ancestor_limits.reverse();
    assert!(ProbeRecord::parse_schema5(&record_bytes(&reordered_ancestors)?).is_none());

    let mut cpu_outside_parent = fixture_record();
    cpu_outside_parent.cpu_ancestor_limits[0].cpuset_cpus_count = 4;
    assert!(ProbeRecord::parse_schema5(&record_bytes(&cpu_outside_parent)?).is_none());

    let mut mismatched_pid = fixture_record();
    mismatched_pid.probe_namespace_pid = 1;
    assert!(ProbeRecord::parse_schema5(&record_bytes(&mismatched_pid)?).is_none());
    Ok(())
}

#[test]
fn schema5_validator_requires_a_directory_root() -> Result<(), HostError> {
    let directory = TempDirectory::new()?;
    let output = directory.path().join("output.json");
    std::fs::write(&output, record_bytes(&fixture_record())?).map_err(|_| HostError::Path)?;
    assert_eq!(validate(&output, &output), Err(HostError::Path));
    Ok(())
}
