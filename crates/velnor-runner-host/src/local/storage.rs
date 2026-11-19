// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use serde::Deserialize;

use crate::error::HostError;
use crate::journal::{StorageProvisioningPlan, StorageProvisioningRole, StorageRole};
use crate::launch::bootstrap::containers::storage_object::tmpfs_options;

use super::layout;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const BYTES_PER_INODE: u64 = 16 * 1024;
const ENGINE_ID: &str = "velnor-local";
const MIN_SIZE_BYTES: u64 = GIB;
const MAX_SIZE_BYTES: u64 = 64 * GIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sizes {
    #[serde(rename = "home_bytes")]
    pub home: u64,
    #[serde(rename = "work_bytes")]
    pub work: u64,
    #[serde(rename = "tmp_bytes")]
    pub tmp: u64,
}

impl Default for Sizes {
    fn default() -> Self {
        Self {
            home: 2 * GIB,
            work: 16 * GIB,
            tmp: 2 * GIB,
        }
    }
}

impl Sizes {
    pub(super) fn checked(self) -> Result<Self, HostError> {
        let allowed = MIN_SIZE_BYTES..=MAX_SIZE_BYTES;
        [self.home, self.work, self.tmp]
            .iter()
            .all(|bytes| allowed.contains(bytes))
            .then_some(self)
            .ok_or(HostError::Config)
    }
}

pub(super) const fn charge_bytes(role: StorageRole, sizes: &Sizes) -> u64 {
    match role {
        StorageRole::RunnerHome => sizes.home,
        StorageRole::RunnerWork => sizes.work,
        StorageRole::RunnerTmp => sizes.tmp,
        StorageRole::DindTmp => 2 * GIB,
        StorageRole::DindData => 16 * GIB,
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

pub(super) const fn charge_inodes(role: StorageRole, sizes: &Sizes) -> u64 {
    charge_bytes(role, sizes) / BYTES_PER_INODE
}

pub(super) fn driver_opts(
    role: StorageRole,
    sizes: &Sizes,
) -> Result<Option<HashMap<String, String>>, HostError> {
    if !on_tmpfs(role) {
        return Ok(None);
    }
    let mut options = tmpfs_options(charge_bytes(role, sizes), charge_inodes(role, sizes))?;
    if matches!(role, StorageRole::RunnerHome | StorageRole::RunnerWork) {
        options
            .get_mut("o")
            .ok_or(HostError::Identity)?
            .push_str(layout::RUNNER_OWNED_ROOT);
    }
    Ok(Some(options))
}

pub(super) fn plan(sequence: u32, sizes: &Sizes) -> StorageProvisioningPlan {
    let roles = StorageRole::PHYSICAL
        .into_iter()
        .map(|role| {
            StorageProvisioningRole::unreserved(
                role,
                ENGINE_ID,
                on_tmpfs(role),
                charge_bytes(role, sizes),
                charge_inodes(role, sizes),
            )
        })
        .collect();
    StorageProvisioningPlan::unreserved(i64::from(sequence), ENGINE_ID, roles)
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
