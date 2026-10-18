use std::collections::BTreeSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::HostError;

use super::observations::{
    StorageMemoryObservation, StoragePoolObservation, StorageSampleIdentity,
};
use super::plan::StorageReservationPlan;
use super::profile::StorageProfile;
use super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium};

const MAX_STORAGE_POOLS: usize = 16;
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub(crate) struct StorageAdmissionSample {
    pub(super) identity: StorageSampleIdentity,
    pub(super) observed_at: Instant,
    pub(super) observed_at_unix_ms: u64,
    pub(super) pools: Vec<StoragePoolObservation>,
    pub(super) memory: Option<StorageMemoryObservation>,
}

impl StorageAdmissionSample {
    pub(crate) fn new(
        identity: StorageSampleIdentity,
        observed_at: Instant,
        pools: Vec<StoragePoolObservation>,
        memory: Option<StorageMemoryObservation>,
    ) -> Result<Self, HostError> {
        let wall_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_millis();
        let observed_at_unix_ms = u64::try_from(wall_time).map_err(|_| HostError::Journal)?;
        Self::new_at(identity, observed_at, observed_at_unix_ms, pools, memory)
    }

    pub(crate) fn new_at(
        identity: StorageSampleIdentity,
        observed_at: Instant,
        observed_at_unix_ms: u64,
        pools: Vec<StoragePoolObservation>,
        memory: Option<StorageMemoryObservation>,
    ) -> Result<Self, HostError> {
        if pools.is_empty() || pools.len() > MAX_STORAGE_POOLS {
            return Err(HostError::Journal);
        }
        if observed_at_unix_ms == 0 || i64::try_from(observed_at_unix_ms).is_err() {
            return Err(HostError::Journal);
        }
        let pool_ids = pools
            .iter()
            .map(|pool| pool.pool_id.as_str())
            .collect::<BTreeSet<_>>();
        if pool_ids.len() != pools.len() {
            return Err(HostError::Journal);
        }
        let filesystem_ids = pools
            .iter()
            .map(|pool| pool.filesystem_instance_digest.as_str())
            .collect::<BTreeSet<_>>();
        if filesystem_ids.len() != pools.len() {
            return Err(HostError::Journal);
        }
        Ok(Self {
            identity,
            observed_at,
            observed_at_unix_ms,
            pools,
            memory,
        })
    }

    pub(crate) fn validate_for(
        &self,
        profile: &StorageProfile,
        plan: &StorageReservationPlan,
        sample_generation_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        base_memory_charge: u64,
        now: Instant,
    ) -> Result<(), HostError> {
        if self.identity.sample_generation_id != sample_generation_id
            || self.identity.profile_generation_id != profile.generation_id()
            || self.identity.profile_digest != profile.profile_digest()
            || self.identity.engine_id != engine_id
            || self.identity.docker_root_digest != docker_root_digest
            || now
                .checked_duration_since(self.observed_at)
                .is_none_or(|age| age > MAX_SAMPLE_AGE)
            || !wall_sample_is_recent(self.observed_at_unix_ms)
        {
            return Err(HostError::Journal);
        }
        profile.validate_plan(plan)?;
        if self.pools.len() != profile.pools().len() {
            return Err(HostError::Journal);
        }
        let mut retained_memory = 0_u64;
        for pool_limit in profile.pools() {
            let observation = self
                .pools
                .iter()
                .find(|pool| pool.pool_id == pool_limit.pool_id)
                .ok_or(HostError::Journal)?;
            if observation.medium != pool_limit.medium
                || observation.total_bytes < pool_limit.max_bytes
            {
                return Err(HostError::Journal);
            }
            match pool_limit.inode_policy {
                StorageInodePolicy::Finite
                    if observation.total_inodes >= pool_limit.max_inodes
                        && observation.total_inodes > 0 => {}
                StorageInodePolicy::DynamicBtrfs
                    if pool_limit.medium == StorageMedium::Disk
                        && observation.filesystem_kind == StorageFilesystemKind::Btrfs
                        && observation.total_inodes == 0
                        && observation.available_inodes == 0 => {}
                _ => return Err(HostError::Journal),
            }
            if pool_limit.medium == StorageMedium::Tmpfs
                && observation.filesystem_kind != StorageFilesystemKind::Tmpfs
            {
                return Err(HostError::Journal);
            }
            let matching_roles = plan
                .roles()
                .iter()
                .filter(|role| role.pool_id == pool_limit.pool_id)
                .collect::<Vec<_>>();
            if matching_roles.iter().any(|role| {
                role.filesystem_instance_digest != observation.filesystem_instance_digest
            }) {
                return Err(HostError::Journal);
            }
            let charged_bytes = matching_roles.iter().try_fold(0_u64, |sum, role| {
                sum.checked_add(role.charge_bytes).ok_or(HostError::Journal)
            })?;
            let charged_inodes = matching_roles.iter().try_fold(0_u64, |sum, role| {
                sum.checked_add(role.charge_inodes)
                    .ok_or(HostError::Journal)
            })?;
            let required_bytes = charged_bytes
                .checked_add(pool_limit.reserve_bytes)
                .ok_or(HostError::Journal)?;
            let required_inodes = charged_inodes
                .checked_add(pool_limit.reserve_inodes)
                .ok_or(HostError::Journal)?;
            let inode_capacity_fits = match pool_limit.inode_policy {
                StorageInodePolicy::Finite => observation.available_inodes >= required_inodes,
                StorageInodePolicy::DynamicBtrfs => true,
                StorageInodePolicy::Unknown => false,
            };
            if observation.available_bytes < required_bytes || !inode_capacity_fits {
                return Err(HostError::Journal);
            }
        }
        for role in plan.roles() {
            retained_memory = retained_memory
                .checked_add(role.retained_memory_bytes)
                .ok_or(HostError::Journal)?;
        }
        if profile
            .pools()
            .iter()
            .any(|pool| pool.medium == StorageMedium::Tmpfs)
        {
            let memory = self.memory.as_ref().ok_or(HostError::Journal)?;
            let total = retained_memory
                .checked_add(base_memory_charge)
                .ok_or(HostError::Journal)?;
            let memory_after = memory
                .ancestor_memory_current_bytes
                .checked_add(total)
                .ok_or(HostError::Journal)?;
            if memory.guest_available_bytes < total
                || memory_after > memory.ancestor_memory_max_bytes
            {
                return Err(HostError::Journal);
            }
        }
        Ok(())
    }

    pub(crate) fn identity(&self) -> &StorageSampleIdentity {
        &self.identity
    }

    pub(crate) fn observed_at_unix_ms(&self) -> u64 {
        self.observed_at_unix_ms
    }

    pub(crate) fn pools(&self) -> &[StoragePoolObservation] {
        &self.pools
    }

    pub(crate) fn memory(&self) -> Option<&StorageMemoryObservation> {
        self.memory.as_ref()
    }
}

fn wall_sample_is_recent(observed_at_unix_ms: u64) -> bool {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    let Ok(now) = u64::try_from(now.as_millis()) else {
        return false;
    };
    now.checked_sub(observed_at_unix_ms)
        .is_some_and(|age| age <= u64::try_from(MAX_SAMPLE_AGE.as_millis()).unwrap_or(u64::MAX))
}

#[derive(Debug, Clone)]
pub(crate) struct StorageAdmission {
    pub(super) plan: StorageReservationPlan,
    pub(super) sample: StorageAdmissionSample,
}

impl StorageAdmission {
    pub(crate) fn new(
        plan: StorageReservationPlan,
        sample: StorageAdmissionSample,
    ) -> Result<Self, HostError> {
        if plan.profile_generation_id() != sample.identity.profile_generation_id
            || plan.profile_digest() != sample.identity.profile_digest
        {
            return Err(HostError::Journal);
        }
        Ok(Self { plan, sample })
    }

    pub(crate) fn plan(&self) -> &StorageReservationPlan {
        &self.plan
    }

    pub(crate) fn sample(&self) -> &StorageAdmissionSample {
        &self.sample
    }
}
