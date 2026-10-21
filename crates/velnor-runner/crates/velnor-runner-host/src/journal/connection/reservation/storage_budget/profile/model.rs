use crate::error::HostError;

use super::super::role::{StorageInodePolicy, StorageMedium, StorageRole};
use super::valid_digest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoragePoolLimit {
    pub(in crate::journal::connection::reservation::storage_budget) pool_id: String,
    pub(in crate::journal::connection::reservation::storage_budget) medium: StorageMedium,
    pub(in crate::journal::connection::reservation::storage_budget) inode_policy:
        StorageInodePolicy,
    pub(in crate::journal::connection::reservation::storage_budget) max_bytes: u64,
    pub(in crate::journal::connection::reservation::storage_budget) max_inodes: u64,
    pub(in crate::journal::connection::reservation::storage_budget) reserve_bytes: u64,
    pub(in crate::journal::connection::reservation::storage_budget) reserve_inodes: u64,
}

impl StoragePoolLimit {
    #[cfg(test)]
    pub(crate) fn new(
        pool_id: String,
        medium: StorageMedium,
        max_bytes: u64,
        max_inodes: u64,
        reserve_bytes: u64,
        reserve_inodes: u64,
    ) -> Result<Self, HostError> {
        Self::new_with_inode_policy(
            pool_id,
            medium,
            StorageInodePolicy::Finite,
            max_bytes,
            max_inodes,
            reserve_bytes,
            reserve_inodes,
        )
    }

    pub(crate) fn new_with_inode_policy(
        pool_id: String,
        medium: StorageMedium,
        inode_policy: StorageInodePolicy,
        max_bytes: u64,
        max_inodes: u64,
        reserve_bytes: u64,
        reserve_inodes: u64,
    ) -> Result<Self, HostError> {
        if !valid_digest(&pool_id)
            || max_bytes == 0
            || reserve_bytes > max_bytes
            || reserve_inodes > max_inodes
            || [max_bytes, max_inodes, reserve_bytes, reserve_inodes]
                .into_iter()
                .any(|value| i64::try_from(value).is_err())
        {
            return Err(HostError::Journal);
        }
        match inode_policy {
            StorageInodePolicy::Finite if max_inodes > 0 => {}
            StorageInodePolicy::DynamicBtrfs
                if medium == StorageMedium::Disk && max_inodes == 0 && reserve_inodes == 0 => {}
            StorageInodePolicy::Unknown if medium == StorageMedium::Disk && max_inodes == 0 => {}
            _ => return Err(HostError::Journal),
        }
        Ok(Self {
            pool_id,
            medium,
            inode_policy,
            max_bytes,
            max_inodes,
            reserve_bytes,
            reserve_inodes,
        })
    }

    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) fn medium(&self) -> StorageMedium {
        self.medium
    }

    pub(crate) fn inode_policy(&self) -> StorageInodePolicy {
        self.inode_policy
    }

    pub(crate) fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub(crate) fn max_inodes(&self) -> u64 {
        self.max_inodes
    }

    pub(crate) fn reserve_bytes(&self) -> u64 {
        self.reserve_bytes
    }

    pub(crate) fn reserve_inodes(&self) -> u64 {
        self.reserve_inodes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageRoleLimit {
    pub(in crate::journal::connection::reservation::storage_budget) role: StorageRole,
    pub(in crate::journal::connection::reservation::storage_budget) pool_id: String,
    pub(in crate::journal::connection::reservation::storage_budget) medium: StorageMedium,
    pub(in crate::journal::connection::reservation::storage_budget) alias_of: Option<StorageRole>,
    pub(in crate::journal::connection::reservation::storage_budget) max_bytes: u64,
    pub(in crate::journal::connection::reservation::storage_budget) max_inodes: u64,
    pub(in crate::journal::connection::reservation::storage_budget) retained_memory_bytes: u64,
}

impl StorageRoleLimit {
    pub(crate) fn new(
        role: StorageRole,
        pool_id: String,
        medium: StorageMedium,
        max_bytes: u64,
        max_inodes: u64,
        retained_memory_bytes: u64,
    ) -> Result<Self, HostError> {
        if !valid_digest(&pool_id)
            || max_bytes == 0
            || max_inodes == 0
            || [max_bytes, max_inodes, retained_memory_bytes]
                .into_iter()
                .any(|value| i64::try_from(value).is_err())
            || (medium == StorageMedium::Disk && retained_memory_bytes != 0)
            || (medium == StorageMedium::Tmpfs && retained_memory_bytes != max_bytes)
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            role,
            pool_id,
            medium,
            alias_of: None,
            max_bytes,
            max_inodes,
            retained_memory_bytes,
        })
    }

    pub(crate) fn new_alias(
        role: StorageRole,
        pool_id: String,
        medium: StorageMedium,
        alias_of: StorageRole,
        max_bytes: u64,
        max_inodes: u64,
    ) -> Result<Self, HostError> {
        if !valid_digest(&pool_id)
            || role == alias_of
            || max_bytes == 0
            || max_inodes == 0
            || i64::try_from(max_bytes).is_err()
            || i64::try_from(max_inodes).is_err()
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            role,
            pool_id,
            medium,
            alias_of: Some(alias_of),
            max_bytes,
            max_inodes,
            retained_memory_bytes: 0,
        })
    }

    pub(crate) fn role(&self) -> StorageRole {
        self.role
    }

    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) fn medium(&self) -> StorageMedium {
        self.medium
    }

    pub(crate) fn alias_of(&self) -> Option<StorageRole> {
        self.alias_of
    }

    pub(crate) fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub(crate) fn max_inodes(&self) -> u64 {
        self.max_inodes
    }

    pub(crate) fn retained_memory_bytes(&self) -> u64 {
        self.retained_memory_bytes
    }
}
