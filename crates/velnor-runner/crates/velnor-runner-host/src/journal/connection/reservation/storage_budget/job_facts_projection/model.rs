//! Canonical job-facts data and encoding.

mod builders;
mod encoding;
mod types;

#[cfg(test)]
mod tests;

pub(super) use builders::{
    CgroupParts, DocumentParts, InstanceBindingParts, StorageRoleParts, cache, cgroup, engine,
    instance_binding, job_facts_mount, linux_device_number, new_document, profile, reservation,
    runner, sample, scope, storage_role,
};
pub(super) use types::{
    Cache, Cgroup, Document, Engine, InstanceBinding, JobFactsMount, Profile, Reservation, Runner,
    RunnerReference, Sample, Scope, StorageRole,
};

/// Immutable bytes and digests for one bound Runner.
pub(crate) struct BoundJobFactsSnapshot {
    bytes: Vec<u8>,
    projection_digest: String,
    physical_binding_digest: String,
    runner_directory_inode: u64,
}

impl BoundJobFactsSnapshot {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn projection_digest(&self) -> &str {
        &self.projection_digest
    }

    pub(crate) fn physical_binding_digest(&self) -> &str {
        &self.physical_binding_digest
    }

    pub(crate) const fn runner_directory_inode(&self) -> u64 {
        self.runner_directory_inode
    }
}
