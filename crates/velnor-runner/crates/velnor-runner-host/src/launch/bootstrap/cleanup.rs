// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::StorageRole;

mod observer;
mod production;
mod request;
mod runtime;
mod validation;

pub(crate) use runtime::{NativeCleanupRuntime, PendingStorageCleanupWitness};
pub use validation::validate_cleanup_observer_record;

#[expect(
    clippy::struct_excessive_bools,
    reason = "the cleanup receipt keeps separate container, volume, mount, and cgroup absence proofs"
)]
pub(crate) struct VerifiedStorageCleanup {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,

    volume_names: Vec<String>,
    instance_binding_digest: String,
    role_inventory: Vec<StorageRole>,
    containers_absent: bool,
    volumes_absent: bool,
    mounts_detached: bool,
    cgroups_absent: bool,
}

impl VerifiedStorageCleanup {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn volume_names(&self) -> &[String] {
        &self.volume_names
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn role_inventory(&self) -> &[StorageRole] {
        &self.role_inventory
    }

    pub(crate) const fn all_owned_refs_absent(&self) -> bool {
        self.containers_absent && self.volumes_absent && self.mounts_detached && self.cgroups_absent
    }
}
