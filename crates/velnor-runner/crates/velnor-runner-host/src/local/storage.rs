// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::{StorageProvisioningPlan, StorageProvisioningRole, StorageRole};

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
    let roles = StorageRole::PHYSICAL
        .into_iter()
        .map(|role| {
            StorageProvisioningRole::unreserved(
                role,
                ENGINE_ID,
                on_tmpfs(role),
                charge_bytes(role),
                charge_inodes(role),
            )
        })
        .collect();
    StorageProvisioningPlan::unreserved(i64::from(sequence), ENGINE_ID, roles)
}
