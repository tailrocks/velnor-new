use super::{InodePolicy, ProbeRecord, fixture_record};

const MEMORY_TOTAL: u64 = 8 * 1024 * 1024 * 1024;

fn record(psi: &str) -> Vec<u8> {
    record_values(5, 20 * GIB, 40 * GIB, 8 * GIB, 125, psi)
}

fn record_values(version: u8, free: u64, total: u64, memory: u64, load: u64, psi: &str) -> Vec<u8> {
    let mut record = fixture_record();
    record.schema_version = version;
    record.docker_root_free_bytes = free;
    record.docker_root_total_bytes = total;
    record.docker_root_available_bytes = free.min(total).min(19 * GIB);
    record.memory_available_bytes = memory;
    record.load_milli = load;
    record.memory_psi_some_avg10_bps = if psi == "null" {
        None
    } else {
        psi.parse().ok()
    };
    let mut bytes = serde_json::to_vec(&record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

const GIB: u64 = 1024 * 1024 * 1024;

#[test]
fn parser_accepts_canonical_record_and_nullable_diagnostic() {
    assert!(ProbeRecord::parse(&record("null"), MEMORY_TOTAL).is_some());
    assert!(ProbeRecord::parse(&record("125"), MEMORY_TOTAL).is_some());
}

#[test]
fn schema_five_record_contains_the_mount_root_source_cpu_and_pid_witness() {
    let record = fixture_record();
    assert_eq!(record.schema_version, 5);
    assert_eq!(record.docker_root_mount_source, "/dev/vdb");
    assert_eq!(record.docker_root_mount_root, "/_data");
    assert_eq!(record.cpu_ancestor_limits.len(), 2);
    assert!(record.probe_cgroup_cpuset_cpus_count > 0);
    assert!(record.probe_host_pid > 0);
    assert_eq!(record.probe_host_pid, record.probe_namespace_pid);

    let value = serde_json::to_value(&record).unwrap_or_default();
    for required in [
        "probe_cgroup_cpuset_cpus_count",
        "cpu_ancestor_limits",
        "probe_host_pid",
        "probe_namespace_pid",
        "docker_root_mount_source",
        "docker_root_mount_root",
    ] {
        assert!(
            value.get(required).is_some(),
            "Producer field is not present: {required}."
        );
    }
    assert_eq!(
        ProbeRecord::parse(&serialize(&record), MEMORY_TOTAL),
        Some(record)
    );
}

#[test]
fn parser_rejects_schema_two_and_pid_authority_failures() {
    let mut schema_two = fixture_record();
    schema_two.schema_version = 2;
    assert!(ProbeRecord::parse(&serialize(&schema_two), MEMORY_TOTAL).is_none());

    let schema_two_wire = br#"{"schema_version":2,"docker_root_mount_target":"/velnor/docker-root","docker_root_mount_id":5,"docker_root_device_major":0,"docker_root_device_minor":38,"docker_root_statx_device_major":0,"docker_root_statx_device_minor":41,"docker_root_fs_type_magic":2435016766,"docker_root_fs_type_name":"btrfs","docker_root_filesystem_id":17,"guest_boot_id":"12345678-1234-1234-1234-123456789abc","docker_root_free_bytes":21474836480,"docker_root_total_bytes":42949672960,"docker_root_available_bytes":20401094656,"docker_root_inode_total":0,"docker_root_inode_free":0,"docker_root_inode_available":0,"docker_root_inode_policy":"dynamic_btrfs","memory_available_bytes":8589934592,"load_milli":125,"memory_psi_some_avg10_bps":null,"probe_cgroup_path":"/system.slice/velnor-resource-probe.service","probe_cgroup_memory_max_bytes":536870912,"probe_cgroup_memory_current_bytes":67108864,"probe_cgroup_swap_max_bytes":0,"probe_cgroup_swap_current_bytes":0,"probe_cgroup_cpu_quota_micros":100000,"probe_cgroup_cpu_period_micros":100000,"probe_cgroup_pids_max":64}"#;
    let mut schema_two_wire = schema_two_wire.to_vec();
    schema_two_wire.push(b'\n');
    assert!(ProbeRecord::parse(&schema_two_wire, MEMORY_TOTAL).is_none());

    let mut zero_host_pid = fixture_record();
    zero_host_pid.probe_host_pid = 0;
    assert!(ProbeRecord::parse(&serialize(&zero_host_pid), MEMORY_TOTAL).is_none());

    let mut zero_namespace_pid = fixture_record();
    zero_namespace_pid.probe_namespace_pid = 0;
    assert!(ProbeRecord::parse(&serialize(&zero_namespace_pid), MEMORY_TOTAL).is_none());

    let mut nested_pid_namespace = fixture_record();
    nested_pid_namespace.probe_namespace_pid = 1;
    assert!(ProbeRecord::parse(&serialize(&nested_pid_namespace), MEMORY_TOTAL).is_none());
}

#[test]
fn parser_accepts_verified_btrfs_mountinfo_and_statx_device_difference() {
    let record = fixture_record();
    assert_eq!(record.docker_root_device_major, 0);
    assert_eq!(record.docker_root_device_minor, 38);
    assert_eq!(record.docker_root_statx_device_major, 0);
    assert_eq!(record.docker_root_statx_device_minor, 41);
    assert!(ProbeRecord::parse(&serialize(&record), MEMORY_TOTAL).is_some());
}

#[test]
fn parser_requires_matching_devices_for_non_btrfs_filesystems() {
    let mut ext4 = fixture_record();
    ext4.docker_root_fs_type_magic = 0xEF53;
    ext4.docker_root_fs_type_name = "ext4".to_owned();
    ext4.docker_root_inode_policy = InodePolicy::Finite;
    ext4.docker_root_inode_total = 100;
    ext4.docker_root_inode_free = 90;
    ext4.docker_root_inode_available = 80;
    assert!(ProbeRecord::parse(&serialize(&ext4), MEMORY_TOTAL).is_none());

    ext4.docker_root_statx_device_minor = ext4.docker_root_device_minor;
    assert!(ProbeRecord::parse(&serialize(&ext4), MEMORY_TOTAL).is_some());

    let mut mislabeled_btrfs = fixture_record();
    mislabeled_btrfs.docker_root_fs_type_name = "ext4".to_owned();
    assert!(ProbeRecord::parse(&serialize(&mislabeled_btrfs), MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_noncanonical_framing_order_and_duplicates() {
    let valid = record("null");
    let mut two_newlines = valid.clone();
    two_newlines.push(b'\n');
    let reordered = serde_json::to_vec(&serde_json::to_value(fixture_record()).unwrap_or_default())
        .unwrap_or_default();
    let duplicate = valid
        .windows(b"\"schema_version\":5".len())
        .position(|part| part == b"\"schema_version\":5")
        .map(|position| {
            let mut bytes = valid.clone();
            bytes.splice(position..position, b"\"schema_version\":5,".iter().copied());
            bytes
        })
        .unwrap_or_default();

    assert!(ProbeRecord::parse(&valid[..valid.len() - 1], MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&two_newlines, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&reordered, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&duplicate, MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_bad_values_missing_fields_and_oversize_output() {
    for invalid in [
        record_values(5, 41, 40, 30, 125, "null"),
        record_values(5, 20 * GIB, 40 * GIB, MEMORY_TOTAL + 1, 125, "null"),
        record_values(5, 20 * GIB, 40 * GIB, 30, 125, "10001"),
        record_values(1, 20 * GIB, 40 * GIB, 30, 125, "null"),
        b"{}\n".to_vec(),
    ] {
        assert!(ProbeRecord::parse(&invalid, MEMORY_TOTAL).is_none());
    }
    let mut missing = fixture_record();
    missing.docker_root_mount_id = 0;
    assert!(ProbeRecord::parse(&serialize(&missing), MEMORY_TOTAL).is_none());
    let mut missing_statx_device = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = missing_statx_device.as_object_mut() {
        object.remove("docker_root_statx_device_minor");
    }
    let mut missing_statx_device = serde_json::to_vec(&missing_statx_device).unwrap_or_default();
    missing_statx_device.push(b'\n');
    assert!(ProbeRecord::parse(&missing_statx_device, MEMORY_TOTAL).is_none());
    let mut missing_mount_root = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = missing_mount_root.as_object_mut() {
        object.remove("docker_root_mount_root");
    }
    let mut missing_mount_root = serde_json::to_vec(&missing_mount_root).unwrap_or_default();
    missing_mount_root.push(b'\n');
    assert!(ProbeRecord::parse(&missing_mount_root, MEMORY_TOTAL).is_none());
    let mut bad_inode_policy = fixture_record();
    bad_inode_policy.docker_root_inode_policy = InodePolicy::Finite;
    assert!(ProbeRecord::parse(&serialize(&bad_inode_policy), MEMORY_TOTAL).is_none());
    let mut bad_cgroup = fixture_record();
    bad_cgroup.probe_cgroup_memory_max_bytes = Some(1);
    assert!(ProbeRecord::parse(&serialize(&bad_cgroup), MEMORY_TOTAL).is_none());
    let mut unexpected = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = unexpected.as_object_mut() {
        object.insert("unexpected".to_owned(), serde_json::Value::from(1));
    }
    let mut unexpected = serde_json::to_vec(&unexpected).unwrap_or_default();
    unexpected.push(b'\n');
    assert!(ProbeRecord::parse(&unexpected, MEMORY_TOTAL).is_none());
    assert!(ProbeRecord::parse(&record("null"), 0).is_none());
    assert!(ProbeRecord::parse(&vec![b' '; 2048], MEMORY_TOTAL).is_none());
    let mut oversize = vec![b' '; 2048];
    oversize.push(b'\n');
    assert!(ProbeRecord::parse(&oversize, MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_old_schemas_and_invalid_mount_source_or_root() {
    let mut old_schema = fixture_record();
    old_schema.schema_version = 4;
    assert!(ProbeRecord::parse(&serialize(&old_schema), MEMORY_TOTAL).is_none());

    let mut empty_source = fixture_record();
    empty_source.docker_root_mount_source.clear();
    assert!(ProbeRecord::parse(&serialize(&empty_source), MEMORY_TOTAL).is_none());

    let mut control_source = fixture_record();
    control_source.docker_root_mount_source.push('\n');
    assert!(ProbeRecord::parse(&serialize(&control_source), MEMORY_TOTAL).is_none());

    let mut relative_root = fixture_record();
    relative_root.docker_root_mount_root = "_data".to_owned();
    assert!(ProbeRecord::parse(&serialize(&relative_root), MEMORY_TOTAL).is_none());

    let mut escaped_root = fixture_record();
    escaped_root.docker_root_mount_root = "/_data/bad\\x".to_owned();
    assert!(ProbeRecord::parse(&serialize(&escaped_root), MEMORY_TOTAL).is_none());
}

#[test]
fn effective_cpu_uses_every_ancestor_and_ignores_only_probe_leaf_quota() {
    let mut record = fixture_record();
    record.cpu_ancestor_limits[0].cpuset_cpus_count = 12;
    record.cpu_ancestor_limits[1].cpuset_cpus_count = 8;
    record.cpu_ancestor_limits[1].cpu_quota_micros = Some(250_000);
    record.cpu_ancestor_limits[1].cpu_period_micros = 100_000;
    record.probe_cgroup_cpuset_cpus_count = 8;
    // The probe leaf quota permits only 100 millicores. Use ancestor limits to measure job capacity.
    assert_eq!(record.probe_cgroup_cpu_quota_micros, Some(10_000));
    assert_eq!(record.effective_cpu_millicores(18), Some(2_500));
    assert_eq!(record.effective_cpu_millicores(2), Some(2_000));
    assert_eq!(record.effective_cpu_millicores(0), None);

    record.cpu_ancestor_limits[1].cpu_quota_micros = Some(125_050);
    assert_eq!(record.effective_cpu_millicores(18), Some(1_250));

    record.cpu_ancestor_limits[1].cpu_quota_micros = None;
    record.cpu_ancestor_limits[1].cpuset_cpus_count = 4;
    record.probe_cgroup_cpuset_cpus_count = 3;
    assert_eq!(record.effective_cpu_millicores(18), Some(3_000));
}

#[test]
fn effective_cpu_rejects_a_changed_probe_leaf_quota() {
    let mut record = fixture_record();
    record.probe_cgroup_cpu_quota_micros = Some(200_000);
    assert_eq!(record.effective_cpu_millicores(8), None);
    record.probe_cgroup_cpu_quota_micros = Some(100_000);
    record.probe_cgroup_cpu_period_micros = 50_000;
    assert_eq!(record.effective_cpu_millicores(8), None);
}

#[test]
fn cpu_ancestry_requires_exact_order_and_closed_cpuset_bounds() {
    let valid = fixture_record();
    assert!(ProbeRecord::parse(&serialize(&valid), MEMORY_TOTAL).is_some());

    let mut missing = valid.clone();
    missing.cpu_ancestor_limits.pop();
    assert!(ProbeRecord::parse(&serialize(&missing), MEMORY_TOTAL).is_none());

    let mut reordered = valid.clone();
    reordered.cpu_ancestor_limits.reverse();
    assert!(ProbeRecord::parse(&serialize(&reordered), MEMORY_TOTAL).is_none());

    let mut outside_parent = valid.clone();
    outside_parent.cpu_ancestor_limits[1].cpuset_cpus_count = 9;
    assert!(ProbeRecord::parse(&serialize(&outside_parent), MEMORY_TOTAL).is_none());

    let mut empty_leaf = valid;
    empty_leaf.probe_cgroup_cpuset_cpus_count = 0;
    assert!(ProbeRecord::parse(&serialize(&empty_leaf), MEMORY_TOTAL).is_none());
}

#[test]
fn parser_rejects_missing_cpu_ancestry_and_unknown_schema_fields() {
    let mut missing = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = missing.as_object_mut() {
        object.remove("cpu_ancestor_limits");
    }
    assert!(ProbeRecord::parse(&serialize_value(&missing), MEMORY_TOTAL).is_none());

    let mut unknown = serde_json::to_value(fixture_record()).unwrap_or_default();
    if let Some(object) = unknown.as_object_mut() {
        object.insert(
            "cpu_authority_override".to_owned(),
            serde_json::Value::from(18),
        );
    }
    assert!(ProbeRecord::parse(&serialize_value(&unknown), MEMORY_TOTAL).is_none());
}

fn serialize(record: &ProbeRecord) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

fn serialize_value(value: &serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}
