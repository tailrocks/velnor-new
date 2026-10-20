use super::super::record::parse_record;
use velnor_resource_probe::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord,
    CleanupObserverRequest, CleanupVolume, NamespaceIdentity, ProcessIdentity,
};

fn process(pid: u32, inode: u64) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        start_ticks: u64::from(pid) * 100,
        namespace_device: [0, 6],
        namespace_inode: inode,
    }
}

fn request() -> CleanupObserverRequest {
    CleanupObserverRequest {
        schema_version: 1,
        operation_id: "a".repeat(32),
        binding_sha256: "b".repeat(64),
        binding: CleanupObserverBinding {
            expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
            docker_root: CleanupDockerRootIdentity {
                target: "/velnor/docker-root".to_owned(),
                mount_id: 10,
                statx_mount_id: 10,
                mount_root: "/".to_owned(),
                mount_device: [0, 9],
                statx_device: [0, 9],
                filesystem_type_magic: 0x0102_1994,
                filesystem_id: 9,
                read_only: true,
            },
            daemon: process(40, 10),
            runner: process(2, 20),
            dind: process(3, 30),
            cgroup_paths: [
                "/system.slice/runner".to_owned(),
                "/system.slice/dind".to_owned(),
            ],
            volume_mountpoints: [
                ("shared-run", "/var/lib/docker/volumes/a/_data"),
                ("runner-home", "/var/lib/docker/volumes/b/_data"),
                ("runner-work", "/var/lib/docker/volumes/c/_data"),
                ("dind-data", "/var/lib/docker/volumes/d/_data"),
            ]
            .into_iter()
            .map(|(role, mountpoint)| CleanupVolume {
                role: role.to_owned(),
                mountpoint: mountpoint.to_owned(),
                mount_device: [8, 1],
                statx_device: [8, 1],
                inode: 10,
            })
            .collect(),
            owned_tmpfs_devices: [
                [0, 1],
                [0, 2],
                [0, 3],
                [0, 4],
                [0, 5],
                [0, 6],
                [0, 7],
                [0, 8],
            ],
        },
    }
}

fn complete_record(request: &CleanupObserverRequest) -> CleanupObserverRecord {
    CleanupObserverRecord {
        schema_version: 1,
        operation_id: request.operation_id.clone(),
        binding_sha256: request.binding_sha256.clone(),
        guest_boot_id: Some(request.binding.expected_guest_boot_id.clone()),
        docker_root: Some(request.binding.docker_root.clone()),
        complete: true,
        owned_references_absent: true,
        issues: Vec::new(),
        task_count: 1,
        descriptor_count: 1,
        namespace_count: 2,
        owned_reference_count: 0,
        absent_volume_mountpoints: request
            .binding
            .volume_mountpoints
            .iter()
            .map(|volume| volume.mountpoint.clone())
            .collect(),
        absent_tmpfs_devices: request.binding.owned_tmpfs_devices.to_vec(),
        absent_mount_namespaces: [&request.binding.runner, &request.binding.dind]
            .into_iter()
            .map(|process| NamespaceIdentity {
                device: process.namespace_device,
                inode: process.namespace_inode,
            })
            .collect(),
        absent_cgroup_paths: Some(request.binding.cgroup_paths.clone()),
    }
}

fn encoded(record: &CleanupObserverRecord) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(record).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

#[test]
fn accepts_only_a_complete_exact_absence_record() {
    let request = request();
    let record = complete_record(&request);
    assert_eq!(parse_record(&encoded(&record), &request), Ok(record));
}

#[test]
fn validates_request_binding_digest_with_shared_wire_rules() {
    let mut value = request();
    value.binding_sha256 = super::super::record::binding_digest(&value.binding).unwrap_or_default();
    assert_eq!(
        velnor_resource_probe::cleanup_observer_wire::validate_request(&value),
        Ok(())
    );

    value.binding_sha256 = "b".repeat(64);
    assert!(velnor_resource_probe::cleanup_observer_wire::validate_request(&value).is_err());
}

#[test]
fn rejects_partial_foreign_and_wrong_operation_records() {
    let request = request();
    let mut record = complete_record(&request);
    record.complete = false;
    assert!(parse_record(&encoded(&record), &request).is_err());

    record = complete_record(&request);
    record.absent_tmpfs_devices.pop();
    assert!(parse_record(&encoded(&record), &request).is_err());

    record = complete_record(&request);
    record.absent_volume_mountpoints[0] = "/foreign/volume".to_owned();
    assert!(parse_record(&encoded(&record), &request).is_err());

    record = complete_record(&request);
    if let Some(namespace) = record.absent_mount_namespaces.first_mut() {
        namespace.inode += 1;
    }
    assert!(parse_record(&encoded(&record), &request).is_err());

    record = complete_record(&request);
    record.absent_cgroup_paths = Some(["/foreign/runner".to_owned(), "system/dind".to_owned()]);
    assert!(parse_record(&encoded(&record), &request).is_err());

    record = complete_record(&request);
    record.operation_id = "c".repeat(32);
    assert!(parse_record(&encoded(&record), &request).is_err());

    record = complete_record(&request);
    record.binding_sha256 = "d".repeat(64);
    assert!(parse_record(&encoded(&record), &request).is_err());
}

#[test]
fn rejects_incomplete_framing_and_guest_identity_changes() {
    let request = request();
    let record = complete_record(&request);
    let mut bytes = encoded(&record);
    bytes.pop();
    assert!(parse_record(&bytes, &request).is_err());

    let mut record = complete_record(&request);
    record.guest_boot_id = Some("foreign-boot".to_owned());
    assert!(parse_record(&encoded(&record), &request).is_err());
}
