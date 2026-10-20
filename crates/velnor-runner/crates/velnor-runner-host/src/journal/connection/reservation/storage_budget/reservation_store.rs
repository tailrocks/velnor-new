//! Durable storage plans and their fresh admission observations.

use std::collections::BTreeMap;

use super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium};

#[derive(Default)]
struct HeldCharges {
    by_pool: BTreeMap<String, HeldPool>,
    memory_bytes: u64,
}

#[derive(Clone)]
struct HeldPool {
    filesystem_instance_digest: String,
    medium: StorageMedium,
    inode_policy: StorageInodePolicy,
    filesystem_kind: StorageFilesystemKind,
    bytes: u64,
    inodes: u64,
}

mod admission;
pub(super) use admission::{admission_fits, matches_existing};
mod sample_history;
pub(super) use sample_history::{
    validate_orphan_roles, validate_orphan_samples, validate_sample_history,
};
mod validation;
pub(super) use validation::{load_plan, validate_rows};
mod writes;
pub(super) use writes::{append_sample, insert_reservation, plan_memory};
