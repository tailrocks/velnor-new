use super::*;

#[test]
fn parser_accepts_canonical_record_and_nullable_diagnostic() {
    assert!(ProbeRecord::parse(&record("null"), MEMORY_TOTAL).is_some());
    assert!(ProbeRecord::parse(&record("125"), MEMORY_TOTAL).is_some());
}

#[test]
fn parser_accepts_verified_btrfs_mountinfo_and_statx_device_difference() {
    let mut record = fixture_record();
    record.docker_root_device_minor = 38;
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
    ext4.docker_root_statx_device_minor = ext4.docker_root_device_minor + 3;
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
        .expect("the canonical fixture has the schema version field");

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

fn serialize(record: &ProbeRecord) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}
