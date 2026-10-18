use crate::error::HostError;

use super::role::{StorageMedium, StorageRole};
use super::{valid_digest, valid_generation};

const MAX_STORAGE_ROWS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageRoleCharge {
    pub(super) role: StorageRole,
    pub(super) pool_id: String,
    pub(super) filesystem_instance_digest: String,
    pub(super) medium: StorageMedium,
    pub(super) charge_bytes: u64,
    pub(super) charge_inodes: u64,
    pub(super) retained_memory_bytes: u64,
}

impl StorageRoleCharge {
    pub(crate) fn new(
        role: StorageRole,
        pool_id: String,
        filesystem_instance_digest: String,
        medium: StorageMedium,
        charge_bytes: u64,
        charge_inodes: u64,
        retained_memory_bytes: u64,
    ) -> Result<Self, HostError> {
        if !valid_digest(&pool_id)
            || !valid_digest(&filesystem_instance_digest)
            || charge_bytes == 0
            || charge_inodes == 0
            || i64::try_from(charge_bytes).is_err()
            || i64::try_from(charge_inodes).is_err()
            || i64::try_from(retained_memory_bytes).is_err()
            || (medium == StorageMedium::Disk && retained_memory_bytes != 0)
            || (medium == StorageMedium::Tmpfs && retained_memory_bytes != charge_bytes)
        {
            return Err(HostError::Journal);
        }
        Ok(Self {
            role,
            pool_id,
            filesystem_instance_digest,
            medium,
            charge_bytes,
            charge_inodes,
            retained_memory_bytes,
        })
    }

    pub(crate) fn role(&self) -> StorageRole {
        self.role
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

    pub(crate) fn charge_bytes(&self) -> u64 {
        self.charge_bytes
    }

    pub(crate) fn charge_inodes(&self) -> u64 {
        self.charge_inodes
    }

    pub(crate) fn retained_memory_bytes(&self) -> u64 {
        self.retained_memory_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageReservationPlan {
    pub(super) profile_generation_id: String,
    pub(super) profile_digest: String,
    pub(super) instance_binding_digest: String,
    pub(super) roles: Vec<StorageRoleCharge>,
}

impl StorageReservationPlan {
    pub(crate) fn new(
        profile_generation_id: String,
        profile_digest: String,
        instance_binding_digest: String,
        mut roles: Vec<StorageRoleCharge>,
    ) -> Result<Self, HostError> {
        roles.sort_by_key(|role| role.role);
        if !valid_generation(&profile_generation_id)
            || !valid_digest(&profile_digest)
            || !valid_digest(&instance_binding_digest)
            || roles.is_empty()
            || roles.len() > MAX_STORAGE_ROWS
        {
            return Err(HostError::Journal);
        }
        let mut role_ids = roles.iter().map(|row| row.role).collect::<Vec<_>>();
        role_ids.sort_unstable();
        if role_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(HostError::Journal);
        }
        Ok(Self {
            profile_generation_id,
            profile_digest,
            instance_binding_digest,
            roles,
        })
    }

    pub(crate) fn roles(&self) -> &[StorageRoleCharge] {
        &self.roles
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn retained_memory_bytes(&self) -> Result<u64, HostError> {
        self.roles.iter().try_fold(0_u64, |sum, role| {
            sum.checked_add(role.retained_memory_bytes)
                .ok_or(HostError::Journal)
        })
    }
}

/// Read-only cache quota projection from one durable reservation and its latest sample.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageCacheProjection {
    reservation_id: i64,
    launch_intent_id: i64,
    profile_generation_id: String,
    profile_digest: String,
    sample_generation_id: String,
    sample_observed_at_unix_ms: u64,
    engine_id: String,
    docker_root_digest: String,
    pool_id: String,
    filesystem_instance_digest: String,
    cache_live_bytes: u64,
    cache_live_inodes: u64,
    cache_stage_bytes: u64,
    cache_stage_inodes: u64,
}

impl StorageCacheProjection {
    pub(super) fn new(
        reservation_id: i64,
        launch_intent_id: i64,
        profile_generation_id: String,
        profile_digest: String,
        sample_generation_id: String,
        sample_observed_at_unix_ms: u64,
        engine_id: String,
        docker_root_digest: String,
        pool_id: String,
        filesystem_instance_digest: String,
        cache_live_bytes: u64,
        cache_live_inodes: u64,
        cache_stage_bytes: u64,
        cache_stage_inodes: u64,
    ) -> Result<Self, HostError> {
        if reservation_id <= 0
            || launch_intent_id <= 0
            || !valid_generation(&profile_generation_id)
            || !valid_digest(&profile_digest)
            || !valid_generation(&sample_generation_id)
            || sample_observed_at_unix_ms == 0
            || i64::try_from(sample_observed_at_unix_ms).is_err()
            || engine_id.is_empty()
            || engine_id.len() > 128
            || !valid_digest(&docker_root_digest)
            || !valid_digest(&pool_id)
            || !valid_digest(&filesystem_instance_digest)
            || cache_live_bytes == 0
            || cache_live_inodes == 0
            || cache_stage_bytes == 0
            || cache_stage_inodes == 0
        {
            return Err(HostError::Journal);
        }
        for value in [
            cache_live_bytes,
            cache_live_inodes,
            cache_stage_bytes,
            cache_stage_inodes,
        ] {
            i64::try_from(value).map_err(|_| HostError::Journal)?;
        }
        Ok(Self {
            reservation_id,
            launch_intent_id,
            profile_generation_id,
            profile_digest,
            sample_generation_id,
            sample_observed_at_unix_ms,
            engine_id,
            docker_root_digest,
            pool_id,
            filesystem_instance_digest,
            cache_live_bytes,
            cache_live_inodes,
            cache_stage_bytes,
            cache_stage_inodes,
        })
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn launch_intent_id(&self) -> i64 {
        self.launch_intent_id
    }

    pub(crate) fn profile_generation_id(&self) -> &str {
        &self.profile_generation_id
    }

    pub(crate) fn profile_digest(&self) -> &str {
        &self.profile_digest
    }

    pub(crate) fn sample_generation_id(&self) -> &str {
        &self.sample_generation_id
    }

    pub(crate) const fn sample_observed_at_unix_ms(&self) -> u64 {
        self.sample_observed_at_unix_ms
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn pool_id(&self) -> &str {
        &self.pool_id
    }

    pub(crate) fn filesystem_instance_digest(&self) -> &str {
        &self.filesystem_instance_digest
    }

    pub(crate) const fn cache_live_bytes(&self) -> u64 {
        self.cache_live_bytes
    }

    pub(crate) const fn cache_live_inodes(&self) -> u64 {
        self.cache_live_inodes
    }

    pub(crate) const fn cache_stage_bytes(&self) -> u64 {
        self.cache_stage_bytes
    }

    pub(crate) const fn cache_stage_inodes(&self) -> u64 {
        self.cache_stage_inodes
    }
}
