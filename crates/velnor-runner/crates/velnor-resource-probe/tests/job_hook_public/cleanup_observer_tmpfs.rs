// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, path::Path};

use velnor_resource_probe::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupVolume, ProcessIdentity,
    descriptor_has_owned_tmpfs, linux_device, mountinfo_line_has_owned_tmpfs,
    observer_runtime_tmpfs_devices,
};

const TMPFS_MAGIC: i64 = 0x0102_1994;

fn sample_binding() -> CleanupObserverBinding {
    let proc_id = |pid, ns_ino| ProcessIdentity {
        pid,
        start_ticks: 100,
        namespace_inode: ns_ino,
        namespace_device: [0, 4],
    };
    let vol = |role: &str, inode| CleanupVolume {
        role: role.to_owned(),
        mountpoint: format!("/var/lib/docker/volumes/{role}/_data"),
        mount_device: [0, 38],
        statx_device: [0, 41],
        inode,
    };
    CleanupObserverBinding {
        expected_guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        metadata_job_facts_inode: Some(500),
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_root: "/docker".to_owned(),
            mount_id: 90,
            statx_mount_id: 90,
            mount_device: [0, 38],
            statx_device: [0, 41],
            filesystem_type_magic: 0x9123_683e,
            filesystem_id: 77,
            read_only: true,
        },
        daemon: proc_id(10, 4001),
        runner: proc_id(20, 4002),
        dind: proc_id(30, 4003),
        cgroup_paths: [
            "/docker/1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            "/docker/2222222222222222222222222222222222222222222222222222222222222222".to_owned(),
        ],
        volume_mountpoints: vec![
            vol("shared-run", 101),
            vol("runner-home", 102),
            vol("runner-work", 103),
            vol("dind-data", 104),
            vol("engine-metadata", 105),
        ],
        owned_tmpfs_devices: [
            [0, 51],
            [0, 52],
            [0, 53],
            [0, 55],
            [0, 56],
            [0, 57],
            [0, 58],
            [0, 59],
        ],
    }
}

#[test]
fn public_cleanup_observer_tmpfs_reference_scans_observer_namespace_and_excludes_only_observer_runtime_mounts()
 {
    let binding = sample_binding();
    let observer_mountinfo = b"\
101 90 0:51 / /dev rw,nosuid - tmpfs tmpfs rw,size=65536k,mode=755\n\
102 101 0:52 / /dev/shm rw,nosuid,nodev - tmpfs shm rw,size=65536k\n\
103 90 0:53 / /run rw,nosuid,nodev - tmpfs tmpfs rw,mode=755\n\
104 90 0:54 / /sys/fs/cgroup ro,nosuid,nodev,noexec - cgroup2 cgroup2 rw\n\
105 90 0:55 / /home/runner rw,nosuid,nodev - tmpfs tmpfs rw,size=2097152k\n";

    let observer_devices =
        observer_runtime_tmpfs_devices(99, Some(([0, 4], 4999)), &binding, observer_mountinfo);
    let expected_observer: HashSet<u64> = [
        linux_device(0, 51),
        linux_device(0, 52),
        linux_device(0, 53),
    ]
    .into_iter()
    .collect();
    assert_eq!(observer_devices, expected_observer);

    assert!(
        observer_runtime_tmpfs_devices(20, Some(([0, 4], 4999)), &binding, observer_mountinfo)
            .is_empty(),
        "runner pid must not be treated as observer"
    );
    assert!(
        observer_runtime_tmpfs_devices(99, Some(([0, 4], 4002)), &binding, observer_mountinfo)
            .is_empty(),
        "runner mount namespace must not be treated as observer"
    );

    let owned_devices: HashSet<u64> = binding
        .owned_tmpfs_devices
        .iter()
        .map(|[maj, min]| linux_device(u64::from(*maj), u64::from(*min)))
        .collect();

    assert_mountinfo_tmpfs_rules(&owned_devices, &observer_devices);
    assert_descriptor_tmpfs_rules(&owned_devices, &observer_devices);
}

fn assert_mountinfo_tmpfs_rules(owned_devices: &HashSet<u64>, observer_devices: &HashSet<u64>) {
    let observer_dev_fields = "101 90 0:51 / /dev rw,nosuid - tmpfs tmpfs rw"
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    let observer_dev_null_bind_fields =
        "106 90 0:51 /null /proc/keys ro,nosuid,nodev,noexec,relatime - tmpfs tmpfs rw"
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
    let leaked_worker_fields = "105 90 0:55 / /home/runner rw,nosuid,nodev - tmpfs tmpfs rw"
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    let leaked_worker_null_bind_fields =
        "107 90 0:55 /null /proc/keys ro,nosuid,nodev,noexec,relatime - tmpfs tmpfs rw"
            .split_ascii_whitespace()
            .collect::<Vec<_>>();

    for (fields, dev, is_observer_task, expected) in [
        (&observer_dev_fields, linux_device(0, 51), true, false),
        (
            &observer_dev_null_bind_fields,
            linux_device(0, 51),
            true,
            false,
        ),
        (
            &observer_dev_null_bind_fields,
            linux_device(0, 51),
            false,
            true,
        ),
        (&observer_dev_fields, linux_device(0, 51), false, true),
        (&leaked_worker_fields, linux_device(0, 55), true, true),
        (
            &leaked_worker_null_bind_fields,
            linux_device(0, 55),
            true,
            true,
        ),
    ] {
        assert_eq!(
            mountinfo_line_has_owned_tmpfs(
                fields,
                dev,
                is_observer_task,
                owned_devices,
                observer_devices,
            ),
            expected,
        );
    }
}

fn assert_descriptor_tmpfs_rules(owned_devices: &HashSet<u64>, observer_devices: &HashSet<u64>) {
    assert!(!descriptor_has_owned_tmpfs(
        true,
        TMPFS_MAGIC,
        [0, 52],
        Path::new("/dev/shm/sem.1"),
        owned_devices,
        observer_devices,
    ));
    assert!(descriptor_has_owned_tmpfs(
        false,
        TMPFS_MAGIC,
        [0, 52],
        Path::new("/dev/shm/sem.1"),
        owned_devices,
        observer_devices,
    ));
    assert!(descriptor_has_owned_tmpfs(
        true,
        TMPFS_MAGIC,
        [0, 55],
        Path::new("/home/runner/leaked"),
        owned_devices,
        observer_devices,
    ));
}
