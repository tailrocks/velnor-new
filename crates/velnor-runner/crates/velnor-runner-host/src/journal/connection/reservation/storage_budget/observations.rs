use crate::error::HostError;

use super::role::{StorageFilesystemKind, StorageMedium};
use super::{valid_digest, valid_generation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoragePoolObservation {
    pub(super) pool_id: String,
    pub(super) filesystem_instance_digest: String,
    pub(super) medium: StorageMedium,
    pub(super) filesystem_kind: StorageFilesystemKind,
    pub(super) total_bytes: u64,
    pub(super) available_bytes: u64,
    pub(super) total_inodes: u64,
    pub(super) available_inodes: u64,
}

impl StoragePoolObservation {
    #[cfg(test)]
    pub(crate) fn new(
        pool_id: String,
        filesystem_instance_digest: String,
        medium: StorageMedium,
        total_bytes: u64,
        available_bytes: u64,
        total_inodes: u64,
        available_inodes: u64,
    ) -> Result<Self, HostError> {
        let filesystem_kind = match medium {
            StorageMedium::Disk => StorageFilesystemKind::Other,
            StorageMedium::Tmpfs => StorageFilesystemKind::Tmpfs,
        };
        Self::new_with_filesystem(
            pool_id,
            filesystem_instance_digest,
            medium,
            filesystem_kind,
            total_bytes,
            available_bytes,
            total_inodes,
            available_inodes,
        )
    }

    pub(crate) fn new_with_filesystem(
        pool_id: String,
        filesystem_instance_digest: String,
        medium: StorageMedium,
        filesystem_kind: StorageFilesystemKind,
        total_bytes: u64,
        available_bytes: u64,
        total_inodes: u64,
        available_inodes: u64,
    ) -> Result<Self, HostError> {
        if !valid_digest(&pool_id)
            || !valid_digest(&filesystem_instance_digest)
            || total_bytes == 0
            || available_bytes > total_bytes
            || available_inodes > total_inodes
            || ((total_inodes == 0) != (available_inodes == 0))
            || [total_bytes, available_bytes, total_inodes, available_inodes]
                .into_iter()
                .any(|value| i64::try_from(value).is_err())
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            pool_id,
            filesystem_instance_digest,
            medium,
            filesystem_kind,
            total_bytes,
            available_bytes,
            total_inodes,
            available_inodes,
        })
    }

    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) fn medium(&self) -> StorageMedium {
        self.medium
    }

    pub(crate) fn filesystem_kind(&self) -> StorageFilesystemKind {
        self.filesystem_kind
    }

    pub(crate) fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub(crate) fn available_bytes(&self) -> u64 {
        self.available_bytes
    }

    pub(crate) fn total_inodes(&self) -> u64 {
        self.total_inodes
    }

    pub(crate) fn available_inodes(&self) -> u64 {
        self.available_inodes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageMemoryObservation {
    pub(super) guest_available_bytes: u64,
    pub(super) ancestor_memory_max_bytes: u64,
    pub(super) ancestor_memory_current_bytes: u64,
    pub(super) ancestor_swap_max_bytes: u64,
    pub(super) ancestor_swap_current_bytes: u64,
}

impl StorageMemoryObservation {
    #[cfg(test)]
    pub(crate) fn new(
        guest_available_bytes: u64,
        ancestor_memory_max_bytes: u64,
        ancestor_memory_current_bytes: u64,
        ancestor_swap_max_bytes: u64,
        ancestor_swap_current_bytes: u64,
    ) -> Result<Self, HostError> {
        if guest_available_bytes == 0
            || ancestor_memory_max_bytes == 0
            || ancestor_memory_current_bytes > ancestor_memory_max_bytes
            || ancestor_swap_current_bytes > ancestor_swap_max_bytes
            || [
                guest_available_bytes,
                ancestor_memory_max_bytes,
                ancestor_memory_current_bytes,
                ancestor_swap_max_bytes,
                ancestor_swap_current_bytes,
            ]
            .into_iter()
            .any(|value| i64::try_from(value).is_err())
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            guest_available_bytes,
            ancestor_memory_max_bytes,
            ancestor_memory_current_bytes,
            ancestor_swap_max_bytes,
            ancestor_swap_current_bytes,
        })
    }

    pub(crate) fn guest_available_bytes(&self) -> u64 {
        self.guest_available_bytes
    }

    pub(crate) fn ancestor_memory_max_bytes(&self) -> u64 {
        self.ancestor_memory_max_bytes
    }

    pub(crate) fn ancestor_memory_current_bytes(&self) -> u64 {
        self.ancestor_memory_current_bytes
    }

    pub(crate) fn ancestor_swap_max_bytes(&self) -> u64 {
        self.ancestor_swap_max_bytes
    }

    pub(crate) fn ancestor_swap_current_bytes(&self) -> u64 {
        self.ancestor_swap_current_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageSampleIdentity {
    pub(super) sample_generation_id: String,
    pub(super) profile_generation_id: String,
    pub(super) profile_digest: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
}

impl StorageSampleIdentity {
    pub(crate) fn new(
        sample_generation_id: String,
        profile_generation_id: String,
        profile_digest: String,
        engine_id: String,
        docker_root_digest: String,
    ) -> Result<Self, HostError> {
        if !valid_generation(&sample_generation_id)
            || !valid_generation(&profile_generation_id)
            || !valid_digest(&profile_digest)
            || engine_id.is_empty()
            || engine_id.len() > 128
            || engine_id.bytes().any(|byte| byte.is_ascii_control())
            || !valid_digest(&docker_root_digest)
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            sample_generation_id,
            profile_generation_id,
            profile_digest,
            engine_id,
            docker_root_digest,
        })
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
}
