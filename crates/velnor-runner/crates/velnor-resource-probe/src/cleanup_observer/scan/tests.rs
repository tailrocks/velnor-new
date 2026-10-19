use super::helpers::{decode_mount_path, device_pair, linux_device, parse_device};

#[test]
fn volume_match_requires_measured_device_and_inode() {
    let request = super::super::tests::request();
    let volume = &request.binding.volume_mountpoints[0];
    assert!(super::mounts::volume_identity_matches(
        volume,
        volume.statx_device,
        volume.inode
    ));
    assert!(!super::mounts::volume_identity_matches(
        volume,
        volume.statx_device,
        volume.inode + 1
    ));
    assert!(!super::mounts::volume_identity_matches(
        volume,
        [volume.statx_device[0], volume.statx_device[1] + 1],
        volume.inode
    ));
}

#[test]
fn mount_decoder_accepts_only_linux_escapes() {
    assert_eq!(decode_mount_path("/a\\040b"), Some("/a b".to_owned()));
    assert_eq!(decode_mount_path("/a\\999"), None);
}

#[test]
fn device_number_conversion_is_reversible() {
    let device = linux_device(259, 65_537);
    assert_eq!(device_pair(device), [259, 65_537]);
    assert_eq!(parse_device("259:65537"), Some(device));
}

#[test]
fn partial_inventory_never_lists_resources_as_absent() {
    let record = super::output::record(
        super::super::tests::request(),
        super::output::ScanEvidence {
            guest_boot_id: Some("01234567-89ab-cdef-0123-456789abcdef".to_owned()),
            docker_root: None,
            issues: vec!["fd_inventory_incomplete".to_owned()],
            task_count: 4,
            descriptor_count: 8,
            namespace_count: 2,
            owned_reference_count: 0,
        },
    );
    assert!(!record.complete);
    assert!(!record.owned_references_absent);
    assert_eq!(record.absent_volume_mountpoints, Vec::<String>::new());
    assert_eq!(record.absent_tmpfs_devices, Vec::<[u32; 2]>::new());
    assert_eq!(
        record.absent_mount_namespaces,
        Vec::<crate::cleanup_observer_wire::NamespaceIdentity>::new()
    );
    assert!(record.absent_cgroup_paths.is_none());
}
