//! Bind one fresh selected-guest sample to the immutable storage profile.

use std::time::Instant;

use sha2::{Digest, Sha256};

use crate::journal::{
    StorageAdmissionSample, StorageFilesystemKind, StorageInodePolicy, StorageMedium,
    StoragePoolObservation, StoragePoolObservationInput, StorageProfile,
    StorageProvisioningAdmission, StorageProvisioningMemoryObservation, StorageRole,
    StorageSampleIdentity,
};

use super::super::record::InodePolicy;
use super::Observation;

const DOCKER_ROOT_TARGET: &str = "/velnor/docker-root";
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const TMPFS_SUPER_MAGIC: u64 = 0x0102_1994;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

impl Observation {
    /// Bind this fresh selected-guest sample to the exact immutable profile.
    pub(super) fn storage_provisioning_admission(
        &self,
        profile: &StorageProfile,
        expected_engine_id: &str,
        expected_docker_root_digest: &str,
    ) -> Option<StorageProvisioningAdmission> {
        if !self.is_recent_at(Instant::now())
            || self.engine_id != expected_engine_id
            || self.docker_root_digest != expected_docker_root_digest
            || disk_pool_identity(self).is_none()
            || self.memory_total_bytes == 0
            || self.record.memory_available_bytes > self.memory_total_bytes
        {
            return None;
        }
        profile.validate_provisioning_contract().ok()?;
        let disk_pools = profile
            .pools()
            .iter()
            .filter(|pool| pool.medium() == StorageMedium::Disk)
            .collect::<Vec<_>>();
        if disk_pools.len() != 1 {
            return None;
        }
        let engine_metadata = profile.role_limit(StorageRole::EngineMetadata).ok()?;
        let pool = profile.pool_limit(engine_metadata.pool_id()).ok()?;
        if disk_pools[0].pool_id() != pool.pool_id() || pool.medium() != StorageMedium::Disk {
            return None;
        }

        let record = &self.record;
        let filesystem_kind = match record.docker_root_fs_type_magic {
            BTRFS_SUPER_MAGIC => StorageFilesystemKind::Btrfs,
            TMPFS_SUPER_MAGIC => StorageFilesystemKind::Tmpfs,
            _ => StorageFilesystemKind::Other,
        };
        let inode_policy = match record.docker_root_inode_policy {
            InodePolicy::Finite => StorageInodePolicy::Finite,
            InodePolicy::DynamicBtrfs => StorageInodePolicy::DynamicBtrfs,
            InodePolicy::Unknown => StorageInodePolicy::Unknown,
        };
        if pool.inode_policy() != inode_policy {
            return None;
        }

        let identity = StorageSampleIdentity::new(
            self.sample_generation_id.clone(),
            profile.generation_id().to_owned(),
            profile.profile_digest().to_owned(),
            self.engine_id.clone(),
            self.docker_root_digest.clone(),
        )
        .ok()?;
        let filesystem_instance_digest =
            filesystem_instance_digest(&FilesystemInstanceDigestInput {
                engine_id: &self.engine_id,
                docker_root_digest: &self.docker_root_digest,
                guest_boot_id: record.guest_boot_id.as_bytes(),
                mount_source: record.docker_root_mount_source.as_bytes(),
                filesystem_type_name: record.docker_root_fs_type_name.as_bytes(),
                filesystem_type_magic: record.docker_root_fs_type_magic,
                mount_device_major: record.docker_root_device_major,
                mount_device_minor: record.docker_root_device_minor,
                statx_device_major: record.docker_root_statx_device_major,
                statx_device_minor: record.docker_root_statx_device_minor,
                filesystem_id: record.docker_root_filesystem_id,
            });
        let pool_observation =
            StoragePoolObservation::new_with_filesystem(StoragePoolObservationInput {
                pool_id: pool.pool_id().to_owned(),
                filesystem_instance_digest,
                medium: StorageMedium::Disk,
                filesystem_kind,
                total_bytes: record.docker_root_total_bytes,
                available_bytes: record.docker_root_available_bytes,
                total_inodes: record.docker_root_inode_total,
                available_inodes: record.docker_root_inode_available,
            })
            .ok()?;
        let sample =
            StorageAdmissionSample::new(identity, self.observed_at, vec![pool_observation], None)
                .ok()?;
        let memory = StorageProvisioningMemoryObservation::new(
            self.memory_total_bytes,
            record.memory_available_bytes,
        )
        .ok()?;
        Some(StorageProvisioningAdmission::new(sample, memory))
    }
}

pub(super) fn disk_pool_identity(observation: &Observation) -> Option<String> {
    let record = &observation.record;
    if record.docker_root_mount_target != DOCKER_ROOT_TARGET
        || record.docker_root_mount_id == 0
        || record.docker_root_filesystem_id == 0
        || record.guest_boot_id.is_empty()
        || (record.docker_root_device_major == 0
            && record.docker_root_device_minor == 0
            && record.docker_root_statx_device_major == 0
            && record.docker_root_statx_device_minor == 0)
    {
        return None;
    }
    Some(filesystem_instance_digest(&FilesystemInstanceDigestInput {
        engine_id: &observation.engine_id,
        docker_root_digest: &observation.docker_root_digest,
        guest_boot_id: record.guest_boot_id.as_bytes(),
        mount_source: record.docker_root_mount_source.as_bytes(),
        filesystem_type_name: record.docker_root_fs_type_name.as_bytes(),
        filesystem_type_magic: record.docker_root_fs_type_magic,
        mount_device_major: record.docker_root_device_major,
        mount_device_minor: record.docker_root_device_minor,
        statx_device_major: record.docker_root_statx_device_major,
        statx_device_minor: record.docker_root_statx_device_minor,
        filesystem_id: record.docker_root_filesystem_id,
    }))
}

pub(in crate::launch) struct FilesystemInstanceDigestInput<'a> {
    pub(in crate::launch) engine_id: &'a str,
    pub(in crate::launch) docker_root_digest: &'a str,
    pub(in crate::launch) guest_boot_id: &'a [u8],
    pub(in crate::launch) mount_source: &'a [u8],
    pub(in crate::launch) filesystem_type_name: &'a [u8],
    pub(in crate::launch) filesystem_type_magic: u64,
    pub(in crate::launch) mount_device_major: u32,
    pub(in crate::launch) mount_device_minor: u32,
    pub(in crate::launch) statx_device_major: u32,
    pub(in crate::launch) statx_device_minor: u32,
    pub(in crate::launch) filesystem_id: u64,
}

pub(in crate::launch) fn filesystem_instance_digest(
    input: &FilesystemInstanceDigestInput<'_>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"velnor-storage-filesystem-instance-v1\0");
    for field in [
        input.engine_id.as_bytes(),
        input.docker_root_digest.as_bytes(),
        input.guest_boot_id,
        input.mount_source,
        input.filesystem_type_name,
    ] {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    hasher.update(input.filesystem_type_magic.to_be_bytes());
    hasher.update(input.mount_device_major.to_be_bytes());
    hasher.update(input.mount_device_minor.to_be_bytes());
    hasher.update(input.statx_device_major.to_be_bytes());
    hasher.update(input.statx_device_minor.to_be_bytes());
    hasher.update(input.filesystem_id.to_be_bytes());
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        digest.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        digest.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    digest
}
