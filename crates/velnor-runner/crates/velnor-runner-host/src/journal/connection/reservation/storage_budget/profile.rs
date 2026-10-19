use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::publication::ControllerPublicationBound;
use super::role::{StorageInodePolicy, StorageMedium, StorageRole};
use super::valid_generation;

const MAX_STORAGE_POOLS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoragePoolLimit {
    pub(super) pool_id: String,
    pub(super) medium: StorageMedium,
    pub(super) inode_policy: StorageInodePolicy,
    pub(super) max_bytes: u64,
    pub(super) max_inodes: u64,
    pub(super) reserve_bytes: u64,
    pub(super) reserve_inodes: u64,
}

impl StoragePoolLimit {
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
    pub(super) role: StorageRole,
    pub(super) pool_id: String,
    pub(super) medium: StorageMedium,
    pub(super) alias_of: Option<StorageRole>,
    pub(super) max_bytes: u64,
    pub(super) max_inodes: u64,
    pub(super) retained_memory_bytes: u64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageProfile {
    pub(super) generation_id: String,
    pub(super) profile_digest: String,
    pub(super) pools: Vec<StoragePoolLimit>,
    pub(super) roles: Vec<StorageRoleLimit>,
    pub(super) controller_publication: Option<ControllerPublicationBound>,
}

impl StorageProfile {
    pub(crate) fn new(
        generation_id: String,
        pools: Vec<StoragePoolLimit>,
        roles: Vec<StorageRoleLimit>,
    ) -> Result<Self, HostError> {
        Self::from_parts(
            generation_id,
            pools,
            roles,
            Some(ControllerPublicationBound::fixed()),
        )
    }

    pub(super) fn new_legacy(
        generation_id: String,
        pools: Vec<StoragePoolLimit>,
        roles: Vec<StorageRoleLimit>,
    ) -> Result<Self, HostError> {
        Self::from_parts(generation_id, pools, roles, None)
    }

    fn from_parts(
        generation_id: String,
        mut pools: Vec<StoragePoolLimit>,
        mut roles: Vec<StorageRoleLimit>,
        controller_publication: Option<ControllerPublicationBound>,
    ) -> Result<Self, HostError> {
        pools.sort_by(|left, right| left.pool_id.cmp(&right.pool_id));
        roles.sort_by_key(|role| role.role);
        validate_profile_shape(&generation_id, &pools, &roles)?;
        validate_pool_ids(&pools)?;
        validate_role_ids(&roles)?;
        validate_roles(&pools, &roles)?;
        validate_alias_totals(&roles)?;
        validate_pool_capacity(&pools, &roles)?;
        if let Some(publication) = &controller_publication {
            publication.validate()?;
        }
        let profile_digest = profile_digest(&pools, &roles, controller_publication.as_ref())?;
        Ok(Self {
            generation_id,
            profile_digest,
            pools,
            roles,
            controller_publication,
        })
    }

    pub(crate) fn generation_id(&self) -> &str {
        &self.generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn pools(&self) -> &[StoragePoolLimit] {
        &self.pools
    }

    pub(crate) fn roles(&self) -> &[StorageRoleLimit] {
        &self.roles
    }

    pub(crate) fn controller_publication(&self) -> Option<&ControllerPublicationBound> {
        self.controller_publication.as_ref()
    }

    pub(super) fn validate_plan(
        &self,
        plan: &super::plan::StorageReservationPlan,
    ) -> Result<(), HostError> {
        if plan.profile_generation_id() != self.generation_id
            || plan.profile_digest() != self.profile_digest
        {
            return Err(HostError::Journal);
        }
        let physical_roles = self
            .roles
            .iter()
            .filter(|role| role.alias_of.is_none())
            .collect::<Vec<_>>();
        if plan.roles.len() != physical_roles.len() {
            return Err(HostError::Journal);
        }
        for charge in &plan.roles {
            let limit = self
                .roles
                .iter()
                .find(|limit| limit.role == charge.role && limit.alias_of.is_none())
                .ok_or(HostError::Journal)?;
            let pool = self
                .pools
                .iter()
                .find(|pool| pool.pool_id == limit.pool_id)
                .ok_or(HostError::Journal)?;
            if charge.pool_id != limit.pool_id
                || charge.medium != limit.medium
                || charge.charge_bytes > limit.max_bytes
                || charge.charge_inodes > limit.max_inodes
                || (pool.inode_policy == StorageInodePolicy::Unknown)
                || charge.retained_memory_bytes > limit.retained_memory_bytes
            {
                return Err(HostError::Journal);
            }
        }
        for alias_target in self
            .roles
            .iter()
            .filter_map(|role| role.alias_of)
            .collect::<std::collections::BTreeSet<_>>()
        {
            let alias_bytes = self
                .roles
                .iter()
                .filter(|role| role.alias_of == Some(alias_target))
                .try_fold(0_u64, |sum, alias| {
                    sum.checked_add(alias.max_bytes).ok_or(HostError::Journal)
                })?;
            let alias_inodes = self
                .roles
                .iter()
                .filter(|role| role.alias_of == Some(alias_target))
                .try_fold(0_u64, |sum, alias| {
                    sum.checked_add(alias.max_inodes).ok_or(HostError::Journal)
                })?;
            let target_charge = plan
                .roles
                .iter()
                .find(|charge| charge.role == alias_target)
                .ok_or(HostError::Journal)?;
            if target_charge.charge_bytes < alias_bytes
                || target_charge.charge_inodes < alias_inodes
            {
                return Err(HostError::Journal);
            }
        }
        Ok(())
    }

    /// Examine the fixed v16 production topology before you record a capacity reservation.
    pub(crate) fn validate_provisioning_contract(&self) -> Result<(), HostError> {
        let publication = self
            .controller_publication
            .as_ref()
            .ok_or(HostError::Journal)?;
        publication.validate()?;
        self.validate_disk_pool_contract()?;
        self.validate_tmpfs_pool_contract()?;
        self.validate_alias_contract()?;
        self.validate_tmpfs_role_limits()?;
        let metadata = self.role_limit(StorageRole::EngineMetadata)?;
        if metadata.max_bytes() < publication.peak_payload_bytes()
            || metadata.max_inodes() < publication.object_count()
        {
            return Err(HostError::Journal);
        }
        let home = self.role_limit(StorageRole::RunnerHome)?;
        if home.max_bytes() < publication.home_inbox_peak_payload_bytes()
            || home.max_inodes() < publication.home_inbox_object_count()
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    fn validate_disk_pool_contract(&self) -> Result<(), HostError> {
        let roles = [
            StorageRole::RunnerRoot,
            StorageRole::DindRoot,
            StorageRole::EngineMetadata,
        ];
        let disk_pool_id = self.role_limit(StorageRole::RunnerRoot)?.pool_id();
        if self.pools.len() != 9 {
            return Err(HostError::Journal);
        }
        for role in roles {
            let limit = self.role_limit(role)?;
            let contract = role.contract().ok_or(HostError::Journal)?;
            if limit.medium() != StorageMedium::Disk
                || contract.medium != StorageMedium::Disk
                || limit.pool_id() != disk_pool_id
            {
                return Err(HostError::Journal);
            }
        }
        let disk_pool = self.pool_limit(disk_pool_id)?;
        if disk_pool.medium() != StorageMedium::Disk
            || disk_pool.inode_policy() == StorageInodePolicy::Unknown
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    fn validate_tmpfs_pool_contract(&self) -> Result<(), HostError> {
        let roles = [
            StorageRole::RunnerHome,
            StorageRole::RunnerWork,
            StorageRole::RunnerTmp,
            StorageRole::RunnerShm,
            StorageRole::SharedRun,
            StorageRole::DindData,
            StorageRole::DindTmp,
            StorageRole::DindShm,
        ];
        let mut pools = std::collections::BTreeSet::new();
        for role in roles {
            let limit = self.role_limit(role)?;
            let contract = role.contract().ok_or(HostError::Journal)?;
            let pool = self.pool_limit(limit.pool_id())?;
            if limit.medium() != StorageMedium::Tmpfs
                || contract.medium != StorageMedium::Tmpfs
                || pool.medium() != StorageMedium::Tmpfs
                || pool.inode_policy() != StorageInodePolicy::Finite
                || !pools.insert(limit.pool_id())
            {
                return Err(HostError::Journal);
            }
        }
        let disk_count = self
            .pools
            .iter()
            .filter(|pool| pool.medium() == StorageMedium::Disk)
            .count();
        let tmpfs_count = self
            .pools
            .iter()
            .filter(|pool| pool.medium() == StorageMedium::Tmpfs)
            .count();
        if disk_count != 1 || tmpfs_count != 8 {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    fn validate_alias_contract(&self) -> Result<(), HostError> {
        let work = self.role_limit(StorageRole::RunnerWork)?;
        for role in [StorageRole::CacheLive, StorageRole::CacheStage] {
            let limit = self.role_limit(role)?;
            if limit.alias_of() != Some(StorageRole::RunnerWork)
                || limit.pool_id() != work.pool_id()
                || limit.medium() != StorageMedium::Tmpfs
                || limit.max_bytes() == 0
                || limit.max_inodes() == 0
                || limit.max_bytes() > work.max_bytes()
                || limit.max_inodes() > work.max_inodes()
            {
                return Err(HostError::Journal);
            }
        }
        Ok(())
    }

    fn validate_tmpfs_role_limits(&self) -> Result<(), HostError> {
        for role in [
            StorageRole::RunnerHome,
            StorageRole::RunnerWork,
            StorageRole::RunnerTmp,
            StorageRole::RunnerShm,
            StorageRole::SharedRun,
            StorageRole::DindData,
            StorageRole::DindTmp,
            StorageRole::DindShm,
        ] {
            let role_limit = self.role_limit(role)?;
            let pool = self.pool_limit(role_limit.pool_id())?;
            if pool.max_bytes() != role_limit.max_bytes()
                || pool.max_inodes() != role_limit.max_inodes()
                || pool.reserve_bytes() != 0
                || pool.reserve_inodes() != 0
            {
                return Err(HostError::Journal);
            }
        }
        Ok(())
    }

    pub(crate) fn role_limit(&self, role: StorageRole) -> Result<&StorageRoleLimit, HostError> {
        self.roles
            .iter()
            .find(|limit| limit.role() == role)
            .ok_or(HostError::Journal)
    }

    pub(crate) fn pool_limit(&self, pool_id: &str) -> Result<&StoragePoolLimit, HostError> {
        self.pools
            .iter()
            .find(|limit| limit.pool_id() == pool_id)
            .ok_or(HostError::Journal)
    }
}

fn validate_profile_shape(
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

fn validate_pool_ids(pools: &[StoragePoolLimit]) -> Result<(), HostError> {
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

fn validate_role_ids(roles: &[StorageRoleLimit]) -> Result<(), HostError> {
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

fn validate_roles(pools: &[StoragePoolLimit], roles: &[StorageRoleLimit]) -> Result<(), HostError> {
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

fn validate_alias_totals(roles: &[StorageRoleLimit]) -> Result<(), HostError> {
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

fn validate_pool_capacity(
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

fn profile_digest(
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

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
