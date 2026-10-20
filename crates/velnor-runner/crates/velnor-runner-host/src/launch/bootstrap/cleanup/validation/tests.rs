use velnor_resource_probe::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupObserverRecord, CleanupVolume,
    NamespaceIdentity, ProcessIdentity,
};

use super::validate_record;

const OPERATION_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BINDING_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn binding() -> CleanupObserverBinding {
    CleanupObserverBinding {
        metadata_job_facts_inode: None,
        expected_guest_boot_id: "guest-boot".to_owned(),
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_id: 10,
            statx_mount_id: 10,
            mount_root: "/".to_owned(),
            mount_device: [1, 2],
            statx_device: [1, 2],
            filesystem_type_magic: 3,
            filesystem_id: 4,
            read_only: true,
        },
        daemon: process(1),
        runner: process(2),
        dind: process(3),
        cgroup_paths: [
            "/sys/fs/cgroup/runner".to_owned(),
            "/sys/fs/cgroup/dind".to_owned(),
        ],
        volume_mountpoints: vec![CleanupVolume {
            role: "shared-run".to_owned(),
            mountpoint: "/var/lib/docker/volumes/shared/_data".to_owned(),
            mount_device: [5, 6],
            statx_device: [5, 6],
            inode: 7,
        }],
        owned_tmpfs_devices: std::array::from_fn(|index| {
            [u32::try_from(index).unwrap_or(u32::MAX), 9]
        }),
    }
}

fn process(inode: u64) -> ProcessIdentity {
    ProcessIdentity {
        pid: 1,
        start_ticks: 2,
        namespace_device: [3, 4],
        namespace_inode: inode,
    }
}

fn sample_record(binding: &CleanupObserverBinding) -> CleanupObserverRecord {
    CleanupObserverRecord {
        schema_version: 1,
        operation_id: OPERATION_ID.to_owned(),
        binding_sha256: BINDING_SHA256.to_owned(),
        guest_boot_id: Some(binding.expected_guest_boot_id.clone()),
        docker_root: Some(binding.docker_root.clone()),
        complete: true,
        owned_references_absent: true,
        issues: Vec::new(),
        task_count: 3,
        descriptor_count: 4,
        namespace_count: 2,
        owned_reference_count: 0,
        absent_volume_mountpoints: binding
            .volume_mountpoints
            .iter()
            .map(|volume| volume.mountpoint.clone())
            .collect(),
        absent_tmpfs_devices: binding.owned_tmpfs_devices.to_vec(),
        absent_mount_namespaces: [&binding.runner, &binding.dind]
            .into_iter()
            .map(|process| NamespaceIdentity {
                device: process.namespace_device,
                inode: process.namespace_inode,
            })
            .collect(),
        absent_cgroup_paths: Some(binding.cgroup_paths.clone()),
    }
}

#[test]
fn exact_absence_lists_are_accepted_by_record_gate() {
    let binding = binding();
    let record = sample_record(&binding);
    assert!(validate_record(&record, &binding, OPERATION_ID, BINDING_SHA256).is_ok());
}

#[test]
fn docker_root_matches_stable_identity_across_mount_namespaces() -> Result<(), &'static str> {
    let binding = binding();
    let mut record = sample_record(&binding);
    let root = record.docker_root.as_mut().ok_or("missing docker root")?;
    root.mount_id = 20;
    root.statx_mount_id = 20;
    assert!(validate_record(&record, &binding, OPERATION_ID, BINDING_SHA256).is_ok());

    let root = record.docker_root.as_mut().ok_or("missing docker root")?;
    root.filesystem_id += 1;
    assert_rejected(&record, &binding);
    let root = record.docker_root.as_mut().ok_or("missing docker root")?;
    root.filesystem_id -= 1;
    root.statx_mount_id += 1;
    assert_rejected(&record, &binding);
    Ok(())
}

#[test]
fn incomplete_or_foreign_record_is_rejected_by_record_gate() {
    let binding = binding();
    let mut record = sample_record(&binding);
    record.issues.push("unreadable_task".to_owned());
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.guest_boot_id = Some("other-boot".to_owned());
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.operation_id.push('c');
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.binding_sha256.push('c');
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.complete = false;
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.schema_version = 2;
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.owned_references_absent = false;
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.owned_reference_count = 1;
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.absent_volume_mountpoints.clear();
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.absent_tmpfs_devices.pop();
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.absent_mount_namespaces.pop();
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.docker_root = None;
    assert_rejected(&record, &binding);
    record = sample_record(&binding);
    record.absent_cgroup_paths = None;
    assert_rejected(&record, &binding);
}

fn assert_rejected(record: &CleanupObserverRecord, binding: &CleanupObserverBinding) {
    assert!(validate_record(record, binding, OPERATION_ID, BINDING_SHA256).is_err());
}
