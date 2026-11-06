// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use super::super::record::{InodePolicy, ProbeRecord};
use super::MAX_OBSERVATION_AGE;

#[derive(Debug, Clone)]
pub(crate) struct ProbeStorageMountWitness {
    mount_source: String,
    mount_root: String,
    mount_target: String,
    mount_id: u64,
    device_major: u32,
    device_minor: u32,
    statx_device_major: u32,
    statx_device_minor: u32,
    filesystem_type_magic: u64,
    filesystem_type_name: String,
    filesystem_id: u64,
    guest_boot_id: String,
    #[expect(
        dead_code,
        reason = "retains byte capacity facts in the immutable filesystem observation receipt"
    )]
    total_bytes: u64,
    #[expect(
        dead_code,
        reason = "retains byte capacity facts in the immutable filesystem observation receipt"
    )]
    free_bytes: u64,
    #[expect(
        dead_code,
        reason = "retains byte capacity facts in the immutable filesystem observation receipt"
    )]
    available_bytes: u64,
    #[expect(
        dead_code,
        reason = "retains inode capacity facts in the immutable filesystem observation receipt"
    )]
    inode_total: u64,
    #[expect(
        dead_code,
        reason = "retains inode capacity facts in the immutable filesystem observation receipt"
    )]
    inode_free: u64,
    #[expect(
        dead_code,
        reason = "retains inode capacity facts in the immutable filesystem observation receipt"
    )]
    inode_available: u64,
    #[expect(
        dead_code,
        reason = "retains the inode policy in the immutable filesystem observation receipt"
    )]
    inode_policy: &'static str,
}

impl ProbeStorageMountWitness {
    fn from_record(record: &ProbeRecord) -> Self {
        let inode_policy = match record.docker_root_inode_policy {
            InodePolicy::Finite => "finite",
            InodePolicy::DynamicBtrfs => "dynamic_btrfs",
            InodePolicy::Unknown => "unknown",
        };
        Self {
            mount_source: record.docker_root_mount_source.clone(),
            mount_root: record.docker_root_mount_root.clone(),
            mount_target: record.docker_root_mount_target.clone(),
            mount_id: record.docker_root_mount_id,
            device_major: record.docker_root_device_major,
            device_minor: record.docker_root_device_minor,
            statx_device_major: record.docker_root_statx_device_major,
            statx_device_minor: record.docker_root_statx_device_minor,
            filesystem_type_magic: record.docker_root_fs_type_magic,
            filesystem_type_name: record.docker_root_fs_type_name.clone(),
            filesystem_id: record.docker_root_filesystem_id,
            guest_boot_id: record.guest_boot_id.clone(),
            total_bytes: record.docker_root_total_bytes,
            free_bytes: record.docker_root_free_bytes,
            available_bytes: record.docker_root_available_bytes,
            inode_total: record.docker_root_inode_total,
            inode_free: record.docker_root_inode_free,
            inode_available: record.docker_root_inode_available,
            inode_policy,
        }
    }

    pub(crate) fn mount_source(&self) -> &str {
        &self.mount_source
    }
    pub(crate) fn mount_root(&self) -> &str {
        &self.mount_root
    }
    pub(crate) fn mount_target(&self) -> &str {
        &self.mount_target
    }
    pub(crate) const fn mount_id(&self) -> u64 {
        self.mount_id
    }
    pub(crate) const fn device_major(&self) -> u32 {
        self.device_major
    }
    pub(crate) const fn device_minor(&self) -> u32 {
        self.device_minor
    }
    pub(crate) const fn statx_device_major(&self) -> u32 {
        self.statx_device_major
    }
    pub(crate) const fn statx_device_minor(&self) -> u32 {
        self.statx_device_minor
    }
    pub(crate) const fn filesystem_type_magic(&self) -> u64 {
        self.filesystem_type_magic
    }
    pub(crate) fn filesystem_type_name(&self) -> &str {
        &self.filesystem_type_name
    }
    pub(crate) const fn filesystem_id(&self) -> u64 {
        self.filesystem_id
    }
    pub(crate) fn guest_boot_id(&self) -> &str {
        &self.guest_boot_id
    }
    #[expect(
        dead_code,
        reason = "exposes full filesystem capacity for future physical-backing checks"
    )]
    pub(crate) const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    #[expect(
        dead_code,
        reason = "exposes full filesystem capacity for future physical-backing checks"
    )]
    pub(crate) const fn free_bytes(&self) -> u64 {
        self.free_bytes
    }
    #[expect(
        dead_code,
        reason = "exposes full filesystem capacity for future physical-backing checks"
    )]
    pub(crate) const fn available_bytes(&self) -> u64 {
        self.available_bytes
    }
    #[expect(
        dead_code,
        reason = "exposes inode capacity for future physical-backing checks"
    )]
    pub(crate) const fn inode_total(&self) -> u64 {
        self.inode_total
    }
    #[expect(
        dead_code,
        reason = "exposes inode capacity for future physical-backing checks"
    )]
    pub(crate) const fn inode_free(&self) -> u64 {
        self.inode_free
    }
    #[expect(
        dead_code,
        reason = "exposes inode capacity for future physical-backing checks"
    )]
    pub(crate) const fn inode_available(&self) -> u64 {
        self.inode_available
    }
    #[expect(
        dead_code,
        reason = "exposes inode policy for future physical-backing checks"
    )]
    pub(crate) const fn inode_policy(&self) -> &'static str {
        self.inode_policy
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StartObservationWitness {
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    admission_binding: ObservationAdmissionBinding,
    pub(super) observed_at: Instant,
    storage_mount: ProbeStorageMountWitness,
}

#[derive(Debug, Clone)]
enum ObservationAdmissionBinding {
    Fresh(String),
    Recovered(String),
}

impl StartObservationWitness {
    pub(super) fn from_observation(
        record: &ProbeRecord,
        engine_id: String,
        docker_root_digest: String,
        sample_generation_id: String,
        observed_at: Instant,
    ) -> Self {
        Self {
            engine_id,
            docker_root_digest,
            admission_binding: ObservationAdmissionBinding::Fresh(sample_generation_id.clone()),
            sample_generation_id,
            observed_at,
            storage_mount: ProbeStorageMountWitness::from_record(record),
        }
    }

    pub(crate) fn storage_mount(&self) -> &ProbeStorageMountWitness {
        &self.storage_mount
    }

    pub(super) fn bind_recovery_sample(&mut self, admission_sample_generation_id: &str) {
        self.admission_binding =
            ObservationAdmissionBinding::Recovered(admission_sample_generation_id.to_owned());
    }

    #[must_use]
    pub(crate) fn is_fresh_for_admission(
        &self,
        engine_id: &str,
        docker_root_digest: &str,
        admission_sample_generation_id: &str,
    ) -> bool {
        let bound_sample = match &self.admission_binding {
            ObservationAdmissionBinding::Fresh(sample)
            | ObservationAdmissionBinding::Recovered(sample) => sample,
        };
        self.engine_id == engine_id
            && self.docker_root_digest == docker_root_digest
            && bound_sample == admission_sample_generation_id
            && self.observed_at.elapsed() <= MAX_OBSERVATION_AGE
    }

    #[must_use]
    pub(crate) fn is_fresh_for(
        &self,
        engine_id: &str,
        docker_root_digest: &str,
        sample_generation_id: &str,
    ) -> bool {
        self.engine_id == engine_id
            && self.docker_root_digest == docker_root_digest
            && self.sample_generation_id == sample_generation_id
            && self.observed_at.elapsed() <= MAX_OBSERVATION_AGE
    }
}
