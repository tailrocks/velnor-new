use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::super::publication::ControllerPublicationBound;
use super::super::role::{StorageInodePolicy, StorageMedium, StorageRole};
use super::super::valid_generation;
use super::{StoragePoolLimit, StorageRoleLimit};

const MAX_STORAGE_POOLS: usize = 16;

pub(super) fn validate_profile_shape(
    generation_id: &str,
    pools: &[StoragePoolLimit],
    roles: &[StorageRoleLimit],
) -> Result<(), HostError> {
    if !valid_generation(generation_id)
        || pools.is_empty()
        || pools.len() > MAX_STORAGE_POOLS
        || roles.len() != StorageRole::ALL.len()
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) fn validate_pool_ids(pools: &[StoragePoolLimit]) -> Result<(), HostError> {
    let mut ids = pools
        .iter()
        .map(|pool| pool.pool_id.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) fn validate_role_ids(roles: &[StorageRoleLimit]) -> Result<(), HostError> {
    let mut role_ids = roles.iter().map(|row| row.role).collect::<Vec<_>>();
    role_ids.sort_unstable();
    let mut expected_role_ids = StorageRole::ALL.to_vec();
    expected_role_ids.sort_unstable();
    if role_ids == expected_role_ids {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

pub(super) fn validate_roles(
    pools: &[StoragePoolLimit],
    roles: &[StorageRoleLimit],
) -> Result<(), HostError> {
    for role in roles {
        let pool = pools
            .iter()
            .find(|pool| pool.pool_id == role.pool_id)
            .ok_or(HostError::Journal)?;
        validate_role_pool_match(pool, role)?;
        validate_role_limits(pool, roles, role)?;
    }
    Ok(())
}

fn validate_role_pool_match(
    pool: &StoragePoolLimit,
    role: &StorageRoleLimit,
) -> Result<(), HostError> {
    if pool.medium == role.medium {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_role_limits(
    pool: &StoragePoolLimit,
    roles: &[StorageRoleLimit],
    role: &StorageRoleLimit,
) -> Result<(), HostError> {
    match role.alias_of {
        Some(alias_of) => validate_alias_role(roles, role, alias_of),
        None => validate_physical_role(pool, role),
    }
}

fn validate_alias_role(
    roles: &[StorageRoleLimit],
    role: &StorageRoleLimit,
    alias_of: StorageRole,
) -> Result<(), HostError> {
    let target = roles
        .iter()
        .find(|candidate| candidate.role == alias_of)
        .ok_or(HostError::Journal)?;
    if matches!(role.role, StorageRole::CacheLive | StorageRole::CacheStage)
        && alias_of == StorageRole::RunnerWork
        && target.alias_of.is_none()
        && target.role != role.role
        && target.pool_id == role.pool_id
        && target.medium == role.medium
        && role.max_bytes <= target.max_bytes
        && role.max_inodes <= target.max_inodes
        && role.retained_memory_bytes == 0
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_physical_role(
    pool: &StoragePoolLimit,
    role: &StorageRoleLimit,
) -> Result<(), HostError> {
    let cache_alias = matches!(role.role, StorageRole::CacheLive | StorageRole::CacheStage);
    let inode_limit_exceeded =
        pool.inode_policy == StorageInodePolicy::Finite && role.max_inodes > pool.max_inodes;
    let retained_memory_invalid = match role.medium {
        StorageMedium::Disk => role.retained_memory_bytes != 0,
        StorageMedium::Tmpfs => role.retained_memory_bytes != role.max_bytes,
    };
    let invalid = cache_alias
        || role.max_bytes == 0
        || role.max_inodes == 0
        || role.max_bytes > pool.max_bytes
        || inode_limit_exceeded
        || retained_memory_invalid
        || (pool.inode_policy != StorageInodePolicy::Finite && role.medium == StorageMedium::Tmpfs);
    if invalid {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) fn validate_alias_totals(roles: &[StorageRoleLimit]) -> Result<(), HostError> {
    for target in roles.iter().filter(|role| role.alias_of.is_none()) {
        let (alias_bytes, alias_inodes) = roles
            .iter()
            .filter(|role| role.alias_of == Some(target.role))
            .try_fold((0_u64, 0_u64), |sum, alias| {
                Ok::<_, HostError>((
                    sum.0
                        .checked_add(alias.max_bytes)
                        .ok_or(HostError::Journal)?,
                    sum.1
                        .checked_add(alias.max_inodes)
                        .ok_or(HostError::Journal)?,
                ))
            })?;
        if alias_bytes > target.max_bytes || alias_inodes > target.max_inodes {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

pub(super) fn validate_pool_capacity(
    pools: &[StoragePoolLimit],
    roles: &[StorageRoleLimit],
) -> Result<(), HostError> {
    for pool in pools {
        let (role_bytes, role_inodes) = roles
            .iter()
            .filter(|role| role.pool_id == pool.pool_id && role.alias_of.is_none())
            .try_fold((0_u64, 0_u64), |sum, role| {
                Ok::<_, HostError>((
                    sum.0
                        .checked_add(role.max_bytes)
                        .ok_or(HostError::Journal)?,
                    sum.1
                        .checked_add(role.max_inodes)
                        .ok_or(HostError::Journal)?,
                ))
            })?;
        let disk_does_not_fit = role_bytes
            .checked_add(pool.reserve_bytes)
            .is_none_or(|charge| charge > pool.max_bytes);
        let finite_inodes_do_not_fit = pool.inode_policy == StorageInodePolicy::Finite
            && role_inodes
                .checked_add(pool.reserve_inodes)
                .is_none_or(|charge| charge > pool.max_inodes);
        if disk_does_not_fit || finite_inodes_do_not_fit {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

pub(super) fn profile_digest(
    pools: &[StoragePoolLimit],
    roles: &[StorageRoleLimit],
    controller_publication: Option<&ControllerPublicationBound>,
) -> Result<String, HostError> {
    let mut pools = pools.iter().collect::<Vec<_>>();
    pools.sort_by(|left, right| left.pool_id.cmp(&right.pool_id));
    let mut roles = roles.iter().collect::<Vec<_>>();
    roles.sort_by_key(|row| row.role);

    let mut digest = Sha256::new();
    digest.update(if controller_publication.is_some() {
        b"velnor-storage-profile-v2\0"
    } else {
        b"velnor-storage-profile-v1\0"
    });
    hash_u64(
        &mut digest,
        u64::try_from(pools.len()).map_err(|_| HostError::Journal)?,
    );
    for pool in pools {
        digest.update(pool.pool_id.as_bytes());
        digest.update([0, pool.medium.as_byte(), pool.inode_policy.as_byte()]);
        hash_u64(&mut digest, pool.max_bytes);
        hash_u64(&mut digest, pool.max_inodes);
        hash_u64(&mut digest, pool.reserve_bytes);
        hash_u64(&mut digest, pool.reserve_inodes);
    }
    hash_u64(
        &mut digest,
        u64::try_from(roles.len()).map_err(|_| HostError::Journal)?,
    );
    for role in roles {
        digest.update(role.role.as_str().as_bytes());
        digest.update([0]);
        digest.update(role.pool_id.as_bytes());
        digest.update([0, role.medium.as_byte()]);
        match role.alias_of {
            Some(alias_of) => {
                digest.update([1]);
                digest.update(alias_of.as_str().as_bytes());
            }
            None => digest.update([0]),
        }
        hash_u64(&mut digest, role.max_bytes);
        hash_u64(&mut digest, role.max_inodes);
        hash_u64(&mut digest, role.retained_memory_bytes);
    }
    if let Some(publication) = controller_publication {
        let policy = publication.digest();
        digest.update(policy.as_bytes());
    }
    Ok(hex_lower(&digest.finalize()))
}

fn hash_u64(digest: &mut Sha256, value: u64) {
    digest.update(value.to_be_bytes());
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
