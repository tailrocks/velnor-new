// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{
    StorageFilesystemKind, StorageInodePolicy, StorageMedium, StorageProvisioningPlan,
    StorageProvisioningRole, StorageRole,
};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const BYTES_PER_INODE: u64 = 16 * 1024;
const ENGINE_ID: &str = "velnor-local";

pub(super) const fn charge_bytes(role: StorageRole) -> u64 {
    match role {
        StorageRole::RunnerHome | StorageRole::RunnerTmp | StorageRole::DindTmp => 2 * GIB,
        StorageRole::RunnerWork | StorageRole::DindData => 16 * GIB,
        StorageRole::RunnerShm | StorageRole::DindShm => 256 * MIB,
        StorageRole::SharedRun | StorageRole::EngineMetadata => 64 * MIB,
        StorageRole::RunnerRoot
        | StorageRole::DindRoot
        | StorageRole::CacheLive
        | StorageRole::CacheStage => GIB,
    }
}

pub(super) const fn on_tmpfs(role: StorageRole) -> bool {
    !matches!(
        role,
        StorageRole::RunnerRoot
            | StorageRole::DindRoot
            | StorageRole::EngineMetadata
            | StorageRole::CacheLive
            | StorageRole::CacheStage
    )
}

pub(super) const fn charge_inodes(role: StorageRole) -> u64 {
    charge_bytes(role) / BYTES_PER_INODE
}

pub(super) fn plan(sequence: u32) -> StorageProvisioningPlan {
    StorageProvisioningPlan {
        reservation_id: i64::from(sequence),
        operation_id: i64::from(sequence),
        engine_id: ENGINE_ID.to_owned(),
        docker_root_digest: "0".repeat(64),
        profile_generation_id: ENGINE_ID.to_owned(),
        profile_digest: "0".repeat(64),
        sample_generation_id: ENGINE_ID.to_owned(),
        roles: StorageRole::PHYSICAL.into_iter().map(role).collect(),
    }
}

fn role(role: StorageRole) -> StorageProvisioningRole {
    let tmpfs = on_tmpfs(role);
    StorageProvisioningRole {
        role,
        pool_id: ENGINE_ID.to_owned(),
        medium: if tmpfs {
            StorageMedium::Tmpfs
        } else {
            StorageMedium::Disk
        },
        filesystem_kind: if tmpfs {
            StorageFilesystemKind::Tmpfs
        } else {
            StorageFilesystemKind::Other
        },
        inode_policy: if tmpfs {
            StorageInodePolicy::Finite
        } else {
            StorageInodePolicy::Unknown
        },
        planned_filesystem_instance_digest: None,
        charge_bytes: charge_bytes(role),
        charge_inodes: charge_inodes(role),
        retained_memory_bytes: if tmpfs { charge_bytes(role) } else { 0 },
    }
}
