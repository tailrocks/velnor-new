use std::collections::BTreeSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::HostError;

use super::observations::{
    StorageMemoryObservation, StoragePoolObservation, StorageSampleIdentity,
};
#[cfg(test)]
use super::plan::StorageReservationPlan;
use super::profile::StorageProfile;
use super::role::StorageMedium;

const MAX_STORAGE_POOLS: usize = 16;
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(30);

mod validation;
use validation::{
    validate_metadata_pool_identity, validate_provisioning_memory,
    validate_provisioning_pool_identity, validate_sample_identity,
};
#[cfg(test)]
use validation::{validate_pool_observations, validate_retained_memory};

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

    #[cfg(test)]
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
        validate_sample_identity(
            self,
            profile,
            sample_generation_id,
            engine_id,
            docker_root_digest,
            now,
        )?;
        profile.validate_plan(plan)?;
        validate_pool_observations(self, profile, plan)?;
        validate_retained_memory(self, profile, plan, base_memory_charge)
    }

    pub(crate) fn identity(&self) -> &StorageSampleIdentity {
        &self.identity
    }

    pub(crate) fn observed_at(&self) -> Instant {
        self.observed_at
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

    pub(super) fn validate_provisioning_for(
        &self,
        profile: &StorageProfile,
        sample_generation_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        base_memory_charge: u64,
        memory: &StorageProvisioningMemoryObservation,
        now: Instant,
    ) -> Result<(), HostError> {
        validate_sample_identity(
            self,
            profile,
            sample_generation_id,
            engine_id,
            docker_root_digest,
            now,
        )?;
        let disk_pools = profile
            .pools()
            .iter()
            .filter(|pool| pool.medium() == StorageMedium::Disk)
            .collect::<Vec<_>>();
        if disk_pools.is_empty() || self.pools.len() != disk_pools.len() {
            return Err(HostError::Journal);
        }
        for limit in disk_pools {
            let observation = self
                .pools
                .iter()
                .find(|pool| pool.pool_id == limit.pool_id())
                .ok_or(HostError::Journal)?;
            validate_provisioning_pool_identity(limit, observation)?;
        }
        let retained_memory = profile
            .roles()
            .iter()
            .filter(|role| role.alias_of().is_none())
            .try_fold(0_u64, |sum, role| {
                sum.checked_add(role.retained_memory_bytes())
                    .ok_or(HostError::Journal)
            })?;
        let required_memory = base_memory_charge
            .checked_add(retained_memory)
            .ok_or(HostError::Journal)?;
        validate_provisioning_memory(profile, memory, required_memory)
    }

    pub(crate) fn validate_metadata_settlement_for(
        &self,
        profile: &StorageProfile,
        sample_generation_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        now: Instant,
    ) -> Result<(), HostError> {
        validate_sample_identity(
            self,
            profile,
            sample_generation_id,
            engine_id,
            docker_root_digest,
            now,
        )?;
        let disk_pools = profile
            .pools()
            .iter()
            .filter(|pool| pool.medium() == StorageMedium::Disk)
            .collect::<Vec<_>>();
        if disk_pools.is_empty() || self.pools.len() != disk_pools.len() {
            return Err(HostError::Journal);
        }
        for limit in disk_pools {
            let observation = self
                .pools
                .iter()
                .find(|pool| pool.pool_id() == limit.pool_id())
                .ok_or(HostError::Journal)?;
            validate_metadata_pool_identity(limit, observation)?;
        }
        Ok(())
    }
}

/// Fresh shared-pool and memory evidence for a capacity reservation created before job filesystems exist.
#[derive(Debug, Clone)]
pub(crate) struct StorageProvisioningAdmission {
    sample: StorageAdmissionSample,
    memory: StorageProvisioningMemoryObservation,
}

impl StorageProvisioningAdmission {
    pub(crate) fn new(
        sample: StorageAdmissionSample,
        memory: StorageProvisioningMemoryObservation,
    ) -> Self {
        Self { sample, memory }
    }

    pub(crate) fn sample(&self) -> &StorageAdmissionSample {
        &self.sample
    }

    pub(crate) fn memory(&self) -> &StorageProvisioningMemoryObservation {
        &self.memory
    }

    pub(crate) fn validate_for(
        &self,
        profile: &StorageProfile,
        sample_generation_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        base_memory_charge: u64,
        now: Instant,
    ) -> Result<(), HostError> {
        self.sample.validate_provisioning_for(
            profile,
            sample_generation_id,
            engine_id,
            docker_root_digest,
            base_memory_charge,
            &self.memory,
            now,
        )
    }
}

/// Selected-guest memory facts from Docker Info and the trusted host probe.
/// Worker ancestor cgroup limits are collected after bootstrap, not inferred here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageProvisioningMemoryObservation {
    guest_total_bytes: u64,
    guest_available_bytes: u64,
}

impl StorageProvisioningMemoryObservation {
    pub(crate) fn new(
        guest_total_bytes: u64,
        guest_available_bytes: u64,
    ) -> Result<Self, HostError> {
        if guest_total_bytes == 0
            || guest_available_bytes == 0
            || guest_available_bytes > guest_total_bytes
            || i64::try_from(guest_total_bytes).is_err()
            || i64::try_from(guest_available_bytes).is_err()
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            guest_total_bytes,
            guest_available_bytes,
        })
    }

    pub(crate) const fn guest_total_bytes(&self) -> u64 {
        self.guest_total_bytes
    }

    pub(crate) const fn guest_available_bytes(&self) -> u64 {
        self.guest_available_bytes
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct StorageAdmission {
    pub(super) plan: StorageReservationPlan,
    pub(super) sample: StorageAdmissionSample,
}

#[cfg(test)]
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
