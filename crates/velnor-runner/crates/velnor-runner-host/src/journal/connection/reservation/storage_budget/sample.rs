use std::collections::BTreeSet;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::HostError;

use super::observations::{
    StorageMemoryObservation, StoragePoolObservation, StorageSampleIdentity,
};
use super::plan::{StorageReservationPlan, StorageRoleCharge};
use super::profile::{StoragePoolLimit, StorageProfile};
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

fn validate_provisioning_pool_identity(
    limit: &StoragePoolLimit,
    observation: &StoragePoolObservation,
) -> Result<(), HostError> {
    if observation.medium() != limit.medium()
        || observation.total_bytes() < limit.max_bytes()
        || observation.available_bytes() < limit.reserve_bytes()
    {
        return Err(HostError::Journal);
    }
    let inode_policy_matches = match limit.inode_policy() {
        StorageInodePolicy::Finite => {
            observation.filesystem_kind() != StorageFilesystemKind::Tmpfs
                && observation.total_inodes() >= limit.max_inodes()
                && observation.total_inodes() > 0
                && observation.available_inodes() >= limit.reserve_inodes()
        }
        StorageInodePolicy::DynamicBtrfs => {
            observation.filesystem_kind() == StorageFilesystemKind::Btrfs
                && observation.total_inodes() == 0
                && observation.available_inodes() == 0
        }
        StorageInodePolicy::Unknown => false,
    };
    if observation.medium() == StorageMedium::Disk && inode_policy_matches {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_metadata_pool_identity(
    limit: &StoragePoolLimit,
    observation: &StoragePoolObservation,
) -> Result<(), HostError> {
    if observation.medium() != StorageMedium::Disk {
        return Err(HostError::Journal);
    }
    let matches = match limit.inode_policy() {
        StorageInodePolicy::Finite => {
            observation.filesystem_kind() != StorageFilesystemKind::Tmpfs
                && observation.total_inodes() > 0
        }
        StorageInodePolicy::DynamicBtrfs => {
            observation.filesystem_kind() == StorageFilesystemKind::Btrfs
                && observation.total_inodes() == 0
                && observation.available_inodes() == 0
        }
        StorageInodePolicy::Unknown => false,
    };
    if matches {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_provisioning_memory(
    profile: &StorageProfile,
    memory: &StorageProvisioningMemoryObservation,
    required_memory_bytes: u64,
) -> Result<(), HostError> {
    let has_tmpfs = profile
        .roles()
        .iter()
        .any(|role| role.alias_of().is_none() && role.medium() == StorageMedium::Tmpfs);
    if !has_tmpfs {
        return Ok(());
    }
    if memory.guest_total_bytes() >= required_memory_bytes
        && memory.guest_available_bytes() >= required_memory_bytes
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_sample_identity(
    sample: &StorageAdmissionSample,
    profile: &StorageProfile,
    sample_generation_id: &str,
    engine_id: &str,
    docker_root_digest: &str,
    now: Instant,
) -> Result<(), HostError> {
    let identity_matches = sample.identity.sample_generation_id == sample_generation_id
        && sample.identity.profile_generation_id == profile.generation_id()
        && sample.identity.profile_digest == profile.profile_digest()
        && sample.identity.engine_id == engine_id
        && sample.identity.docker_root_digest == docker_root_digest;
    let sample_is_fresh = now
        .checked_duration_since(sample.observed_at)
        .is_some_and(|age| age <= MAX_SAMPLE_AGE)
        && wall_sample_is_recent(sample.observed_at_unix_ms);
    if identity_matches && sample_is_fresh {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_pool_observations(
    sample: &StorageAdmissionSample,
    profile: &StorageProfile,
    plan: &StorageReservationPlan,
) -> Result<(), HostError> {
    if sample.pools.len() != profile.pools().len() {
        return Err(HostError::Journal);
    }
    for limit in profile.pools() {
        let observation = sample
            .pools
            .iter()
            .find(|pool| pool.pool_id == limit.pool_id)
            .ok_or(HostError::Journal)?;
        validate_pool_observation(limit, observation, plan)?;
    }
    Ok(())
}

fn validate_pool_observation(
    limit: &StoragePoolLimit,
    observation: &StoragePoolObservation,
    plan: &StorageReservationPlan,
) -> Result<(), HostError> {
    validate_pool_identity(limit, observation)?;
    let matching_roles = plan
        .roles()
        .iter()
        .filter(|role| role.pool_id == limit.pool_id)
        .collect::<Vec<_>>();
    if matching_roles
        .iter()
        .any(|role| role.filesystem_instance_digest != observation.filesystem_instance_digest)
    {
        return Err(HostError::Journal);
    }
    validate_pool_capacity(limit, observation, &matching_roles)
}

fn validate_pool_identity(
    limit: &StoragePoolLimit,
    observation: &StoragePoolObservation,
) -> Result<(), HostError> {
    let inode_policy_matches = match limit.inode_policy {
        StorageInodePolicy::Finite => {
            observation.total_inodes >= limit.max_inodes && observation.total_inodes > 0
        }
        StorageInodePolicy::DynamicBtrfs => {
            limit.medium == StorageMedium::Disk
                && observation.filesystem_kind == StorageFilesystemKind::Btrfs
                && observation.total_inodes == 0
                && observation.available_inodes == 0
        }
        StorageInodePolicy::Unknown => false,
    };
    let filesystem_matches = limit.medium != StorageMedium::Tmpfs
        || observation.filesystem_kind == StorageFilesystemKind::Tmpfs;
    if observation.medium == limit.medium
        && observation.total_bytes >= limit.max_bytes
        && inode_policy_matches
        && filesystem_matches
    {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_pool_capacity(
    limit: &StoragePoolLimit,
    observation: &StoragePoolObservation,
    roles: &[&StorageRoleCharge],
) -> Result<(), HostError> {
    let charged_bytes = roles.iter().try_fold(0_u64, |sum, role| {
        sum.checked_add(role.charge_bytes).ok_or(HostError::Journal)
    })?;
    let charged_inodes = roles.iter().try_fold(0_u64, |sum, role| {
        sum.checked_add(role.charge_inodes)
            .ok_or(HostError::Journal)
    })?;
    let required_bytes = charged_bytes
        .checked_add(limit.reserve_bytes)
        .ok_or(HostError::Journal)?;
    let required_inodes = charged_inodes
        .checked_add(limit.reserve_inodes)
        .ok_or(HostError::Journal)?;
    let inode_capacity_fits = match limit.inode_policy {
        StorageInodePolicy::Finite => observation.available_inodes >= required_inodes,
        StorageInodePolicy::DynamicBtrfs => true,
        StorageInodePolicy::Unknown => false,
    };
    if observation.available_bytes >= required_bytes && inode_capacity_fits {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}

fn validate_retained_memory(
    sample: &StorageAdmissionSample,
    profile: &StorageProfile,
    plan: &StorageReservationPlan,
    base_memory_charge: u64,
) -> Result<(), HostError> {
    let has_tmpfs = profile
        .pools()
        .iter()
        .any(|pool| pool.medium == StorageMedium::Tmpfs);
    if !has_tmpfs {
        return Ok(());
    }
    let retained_memory = plan.roles().iter().try_fold(0_u64, |sum, role| {
        sum.checked_add(role.retained_memory_bytes)
            .ok_or(HostError::Journal)
    })?;
    let memory = sample.memory.as_ref().ok_or(HostError::Journal)?;
    let total = retained_memory
        .checked_add(base_memory_charge)
        .ok_or(HostError::Journal)?;
    let memory_after = memory
        .ancestor_memory_current_bytes
        .checked_add(total)
        .ok_or(HostError::Journal)?;
    if memory.guest_available_bytes >= total && memory_after <= memory.ancestor_memory_max_bytes {
        Ok(())
    } else {
        Err(HostError::Journal)
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
