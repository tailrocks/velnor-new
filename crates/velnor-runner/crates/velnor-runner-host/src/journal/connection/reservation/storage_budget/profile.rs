use crate::error::HostError;

use super::publication::ControllerPublicationBound;
use super::role::{StorageInodePolicy, StorageMedium, StorageRole};

mod model;
pub(crate) use model::{StoragePoolLimit, StorageRoleLimit};
mod validation;
use validation::{
    profile_digest, validate_alias_totals, validate_pool_capacity, validate_pool_ids,
    validate_profile_shape, validate_role_ids, validate_roles,
};

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

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
