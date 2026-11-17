// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Pair, TestResult, seed};
use crate::journal::Journal;
use sha2::{Digest, Sha256};
use velnor_resource_probe::cleanup_observer_wire::{
    CleanupDockerRootIdentity, CleanupObserverBinding, CleanupVolume, ProcessIdentity,
};

pub(super) fn digest(value: &[u8]) -> String {
    crate::digest_hex::lowercase_hex(&Sha256::digest(value))
}

pub(super) fn binding(pair: Pair) -> CleanupObserverBinding {
    let process = |pid, namespace_inode| ProcessIdentity {
        pid,
        start_ticks: 100,
        namespace_inode,
        namespace_device: [0, 4],
    };
    CleanupObserverBinding {
        expected_guest_boot_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        metadata_job_facts_inode: Some(500),
        docker_root: CleanupDockerRootIdentity {
            target: "/velnor/docker-root".to_owned(),
            mount_root: "/docker".to_owned(),
            mount_id: 90,
            statx_mount_id: 90,
            mount_device: [0, 38],
            statx_device: [0, 38],
            filesystem_type_magic: 0x9123_683e,
            filesystem_id: 77,
            read_only: true,
        },
        daemon: process(10, 4001),
        runner: process(20, 4002),
        dind: process(30, 4003),
        cgroup_paths: [
            format!("/docker/{}", pair.container_id("runner")),
            format!("/docker/{}", pair.container_id("dind")),
        ],
        volume_mountpoints: pair
            .volumes()
            .into_iter()
            .enumerate()
            .map(|(index, (name, role))| CleanupVolume {
                role: role.to_owned(),
                mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
                mount_device: [0, 38],
                statx_device: [0, 38],
                inode: 101 + u64::try_from(index).expect("volume index"),
            })
            .collect(),
        owned_tmpfs_devices: [
            [0, 51],
            [0, 52],
            [0, 53],
            [0, 54],
            [0, 55],
            [0, 56],
            [0, 57],
            [0, 58],
        ],
    }
}

pub(super) async fn install(journal: &Journal, pair: Pair) -> TestResult<()> {
    let json = serde_json::to_string(&binding(pair))?;
    seed::unguarded(journal,&[format!("UPDATE capacity_reservation_cleanup_observer_witnesses SET binding_json='{json}',binding_sha256='{}' WHERE reservation_id={}",digest(json.as_bytes()),pair.reservation)]).await
}
