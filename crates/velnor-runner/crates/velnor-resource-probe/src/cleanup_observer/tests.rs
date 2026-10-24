use sha2::{Digest, Sha256};

use super::{
    CleanupObserverBinding, CleanupObserverRequest, CleanupVolume, ProcessIdentity, lower_hex,
    validate_request,
};

pub(super) fn request() -> CleanupObserverRequest {
    let binding = CleanupObserverBinding {
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        metadata_job_facts_inode: None,
        docker_root: super::CleanupDockerRootIdentity {
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
        daemon: process(40, 101, 10),
        runner: process(2, 202, 20),
        dind: process(3, 303, 30),
        cgroup_paths: [
            "/system.slice/velnor-runner".to_owned(),
            "/system.slice/velnor-dind".to_owned(),
        ],
        volume_mountpoints: [
            ("shared-run", "/var/lib/docker/volumes/a/_data"),
            ("runner-home", "/var/lib/docker/volumes/b/_data"),
            ("runner-work", "/var/lib/docker/volumes/c/_data"),
            ("dind-data", "/var/lib/docker/volumes/d/_data"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (role, mountpoint))| CleanupVolume {
            role: role.to_owned(),
            mountpoint: mountpoint.to_owned(),
            mount_device: [8, 1],
            statx_device: [8, 1],
            inode: u64::try_from(index).unwrap_or_default().saturating_add(10),
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
    };
    let bytes = serde_json::to_vec(&binding).expect("serialize binding");
    CleanupObserverRequest {
        schema_version: 1,
        operation_id: "0123456789abcdef0123456789abcdef".to_owned(),
        binding_sha256: lower_hex(&Sha256::digest(bytes)),
        binding,
    }
}

fn process(pid: u32, inode: u64, minor: u32) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        start_ticks: u64::from(pid) * 100,
        namespace_device: [0, 6],
        namespace_inode: inode + u64::from(minor),
    }
}

#[test]
fn accepts_a_bound_cleanup_request() {
    assert!(validate_request(&request()).is_ok());
}

#[test]
fn rejects_a_changed_binding_digest() {
    let mut value = request();
    value.binding.daemon.pid = 42;
    assert!(validate_request(&value).is_err());
}

#[test]
fn rejects_a_duplicate_temporary_filesystem_device() {
    let mut value = request();
    value.binding.owned_tmpfs_devices[7] = value.binding.owned_tmpfs_devices[0];
    resign(&mut value);
    assert!(validate_request(&value).is_err());
}

#[test]
fn rejects_an_untrusted_volume_role() {
    let mut value = request();
    value.binding.volume_mountpoints[0].role = "foreign-volume".to_owned();
    resign(&mut value);
    assert!(validate_request(&value).is_err());
}

#[test]
fn rejects_a_path_with_parent_traversal() {
    let mut value = request();
    value.binding.volume_mountpoints[0].mountpoint = "/var/lib/docker/../foreign".to_owned();
    resign(&mut value);
    assert!(validate_request(&value).is_err());
}

fn resign(value: &mut CleanupObserverRequest) {
    let bytes = serde_json::to_vec(&value.binding).expect("serialize binding");
    value.binding_sha256 = lower_hex(&Sha256::digest(bytes));
}
