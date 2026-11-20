// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_resource_probe::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupVolume, ProcessIdentity,
    owned_mountinfo_projection,
};

const RUNNER_NS: u64 = 4_026_532_605;
const DIND_NS: u64 = 4_026_532_617;
const DAEMON_NS: u64 = 4_026_532_288;

fn process(pid: u32, namespace_inode: u64) -> ProcessIdentity {
    ProcessIdentity {
        pid,
        start_ticks: 1,
        namespace_device: [0, 5],
        namespace_inode,
    }
}

fn binding() -> CleanupObserverBinding {
    CleanupObserverBinding {
        expected_guest_boot_id: "ff20bcb1-748b-4fc5-a4cc-5e0657b9fe45".to_owned(),
        metadata_job_facts_inode: None,
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_id: 437,
            statx_mount_id: 437,
            mount_root: "/docker".to_owned(),
            mount_device: [0, 38],
            statx_device: [0, 41],
            filesystem_type_magic: 0x9123_683e,
            filesystem_id: 7,
            read_only: true,
        },
        daemon: process(13, DAEMON_NS),
        runner: process(2_662_211, RUNNER_NS),
        dind: process(2_661_965, DIND_NS),
        cgroup_paths: ["/docker/a".to_owned(), "/docker/b".to_owned()],
        volume_mountpoints: vec![CleanupVolume {
            role: "runner-home".to_owned(),
            mountpoint: "/var/lib/docker/volumes/w1-home/_data".to_owned(),
            mount_device: [0, 77],
            statx_device: [0, 77],
            inode: 1,
        }],
        owned_tmpfs_devices: [[0, 90]; 8],
    }
}

const BASE: &str = "\
24 1 0:21 / /proc rw - proc proc rw
310 24 0:77 / /var/lib/docker/volumes/w1-home/_data rw - ext4 /dev/vda rw
311 24 0:4 net:[4026532605] /run/docker/netns/runner rw - nsfs nsfs rw
";

fn projection(mountinfo: &str) -> Vec<u8> {
    owned_mountinfo_projection(mountinfo.as_bytes(), &binding())
}

#[test]
fn a_mount_of_an_unrelated_container_does_not_change_the_projection() {
    let churned = format!(
        "{BASE}\
512 24 0:200 / /var/lib/docker/volumes/other/_data rw - ext4 /dev/vda rw
513 24 0:4 net:[4026539999] /run/docker/netns/other rw - nsfs nsfs rw
514 24 0:201 / /run/other rw - tmpfs tmpfs rw
"
    );
    assert_eq!(projection(BASE), projection(&churned));
}

#[test]
fn a_new_mount_of_an_owned_volume_changes_the_projection() {
    let reattached = format!("{BASE}512 24 0:77 / /mnt/stale rw - ext4 /dev/vda rw\n");
    assert_ne!(projection(BASE), projection(&reattached));
}

#[test]
fn a_new_bind_of_an_owned_namespace_changes_the_projection() {
    let reattached = format!("{BASE}512 24 0:4 mnt:[4026532617] /run/stale rw - nsfs nsfs rw\n");
    assert_ne!(projection(BASE), projection(&reattached));
}

#[test]
fn an_owned_mount_that_disappears_changes_the_projection() {
    let gone = BASE
        .lines()
        .filter(|line| !line.contains("0:77"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(projection(BASE), projection(&gone));
}
