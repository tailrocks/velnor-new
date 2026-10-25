use super::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupVolume, ProcessIdentity,
    docker_root_matches, validate_binding,
};

fn process(pid: u32, inode: u64) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        start_ticks: u64::from(pid) * 100,
        namespace_device: [0, 6],
        namespace_inode: inode,
    }
}

fn binding() -> CleanupObserverBinding {
    CleanupObserverBinding {
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        metadata_job_facts_inode: None,
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
            inode: u64::try_from(index).unwrap_or_default() + 10,
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
    }
}

#[test]
fn validates_shared_capture_binding() {
    assert_eq!(validate_binding(&binding()), Ok(()));
}

#[test]
fn initialized_child_inode_is_hashable_and_legacy_bytes_stay_unchanged() {
    let legacy = binding();
    let legacy_bytes =
        serde_json::to_vec(&legacy).expect("The legacy binding must serialize to JSON.");
    assert!(
        !legacy_bytes
            .windows(b"metadata_job_facts_inode".len())
            .any(|window| window == b"metadata_job_facts_inode")
    );
    let decoded: CleanupObserverBinding =
        serde_json::from_slice(&legacy_bytes).expect("The legacy binding must decode from JSON.");
    assert_eq!(
        serde_json::to_vec(&decoded).expect("The legacy binding must re-encode to JSON."),
        legacy_bytes
    );

    let mut current = legacy;
    current.metadata_job_facts_inode = Some(65);
    let current_bytes =
        serde_json::to_vec(&current).expect("The current binding must serialize to JSON.");
    assert!(
        current_bytes
            .windows(b"metadata_job_facts_inode".len())
            .any(|window| window == b"metadata_job_facts_inode")
    );
    let decoded: CleanupObserverBinding =
        serde_json::from_slice(&current_bytes).expect("The current binding must decode from JSON.");
    assert_eq!(decoded.metadata_job_facts_inode, Some(65));
}

#[test]
fn rejects_wrong_guest_boot_root_and_process_owner() {
    let mut value = binding();
    value.expected_guest_boot_id = "bad".to_owned();
    assert_eq!(validate_binding(&value), Err("cleanup_request_identity"));

    value = binding();
    value.docker_root.target = "/foreign/root".to_owned();
    assert_eq!(
        validate_binding(&value),
        Err("cleanup_docker_root_identity")
    );

    value = binding();
    value.runner.pid = value.daemon.pid;
    assert_eq!(validate_binding(&value), Err("cleanup_binding_shape"));
}

#[test]
fn compares_stable_root_and_checks_local_identity_pairs() {
    let expected = binding().docker_root;
    let mut observed = expected.clone();
    observed.mount_id += 20;
    observed.statx_mount_id += 20;
    assert!(docker_root_matches(&expected, &observed));

    observed.filesystem_id += 1;
    assert!(!docker_root_matches(&expected, &observed));

    observed = expected.clone();
    observed.statx_mount_id += 1;
    assert!(!docker_root_matches(&expected, &observed));

    observed = expected.clone();
    observed.statx_device[1] += 1;
    assert!(!docker_root_matches(&expected, &observed));
}
