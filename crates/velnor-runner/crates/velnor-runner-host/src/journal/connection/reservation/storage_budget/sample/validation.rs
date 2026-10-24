use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::error::HostError;

use super::super::observations::StoragePoolObservation;
#[cfg(test)]
use super::super::plan::{StorageReservationPlan, StorageRoleCharge};
use super::super::profile::{StoragePoolLimit, StorageProfile};
use super::super::role::{StorageFilesystemKind, StorageInodePolicy, StorageMedium};
use super::{
    MAX_SAMPLE_AGE, StorageAdmissionSample, StorageProvisioningMemoryObservation,
    StorageSampleValidationIdentity,
};

pub(super) fn validate_provisioning_pool_identity(
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

pub(super) fn validate_metadata_pool_identity(
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

pub(super) fn validate_provisioning_memory(
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

pub(super) fn validate_sample_identity(
    sample: &StorageAdmissionSample,
    profile: &StorageProfile,
    expected_identity: &StorageSampleValidationIdentity<'_>,
    now: Instant,
) -> Result<(), HostError> {
    let identity_matches = sample.identity.sample_generation_id
        == expected_identity.sample_generation_id()
        && sample.identity.profile_generation_id == profile.generation_id()
        && sample.identity.profile_digest == profile.profile_digest()
        && sample.identity.engine_id == expected_identity.engine_id()
        && sample.identity.docker_root_digest == expected_identity.docker_root_digest();
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

#[cfg(test)]
pub(super) fn validate_pool_observations(
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
pub(super) fn validate_retained_memory(
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
        .ancestor_memory_current
        .checked_add(total)
        .ok_or(HostError::Journal)?;
    if memory.guest_available >= total && memory_after <= memory.ancestor_memory_max {
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
