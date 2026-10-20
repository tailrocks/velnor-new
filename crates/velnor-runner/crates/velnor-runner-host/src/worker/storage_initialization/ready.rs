use crate::{
    error::HostError,
    journal::{
        StorageInitializationClaim, VerifiedMetadataInitialization,
        VerifiedMetadataInitializationHandoff,
    },
};

use super::{ObservedStorageInitialization, PendingStorageInitialization};

/// Final, durable storage layout facts retained after the initializer is removed.
pub(crate) struct VerifiedStorageInitializationReady {
    final_readiness: VerifiedMetadataInitialization,
    observed: ObservedStorageInitialization,
}

impl VerifiedStorageInitializationReady {
    pub(super) fn after_final_readiness(
        pending: &PendingStorageInitialization,
        final_readiness: VerifiedMetadataInitialization,
    ) -> Result<Self, HostError> {
        Self::from_parts(
            pending.claim(),
            pending.metadata_handoff(),
            pending.observed().clone(),
            final_readiness,
        )
    }

    #[cfg(test)]
    pub(crate) fn test_after_final_readiness(
        claim: &StorageInitializationClaim,
        handoff: &VerifiedMetadataInitializationHandoff,
        observed: ObservedStorageInitialization,
        final_readiness: VerifiedMetadataInitialization,
    ) -> Result<Self, HostError> {
        Self::from_parts(claim, handoff, observed, final_readiness)
    }

    fn from_parts(
        claim: &StorageInitializationClaim,
        handoff: &VerifiedMetadataInitializationHandoff,
        observed: ObservedStorageInitialization,
        final_readiness: VerifiedMetadataInitialization,
    ) -> Result<Self, HostError> {
        if final_readiness.connection_id() != handoff.connection_id()
            || final_readiness.reservation_id() != claim.reservation_id()
            || final_readiness.operation_id() != claim.operation_id()
            || final_readiness.engine_id() != claim.engine_id()
            || final_readiness.docker_root_digest() != claim.docker_root_digest()
            || final_readiness.operation_key() != claim.operation_key()
            || final_readiness.layout_version() != claim.layout_version()
            || final_readiness.metadata_volume_name()
                != claim.metadata_volume_name().ok_or(HostError::Identity)?
            || final_readiness.metadata_volume_id()
                != claim.metadata_volume_id().ok_or(HostError::Identity)?
            || final_readiness.layout_digest() != handoff.layout_digest()
            || observed.layout_digest() != handoff.layout_digest()
            || observed.ready_digest() != handoff.ready_digest()
            || handoff.reservation_id() != claim.reservation_id()
            || handoff.operation_id() != claim.operation_id()
            || handoff.generation() != claim.generation()
            || handoff.operation_key() != claim.operation_key()
            || handoff.engine_id() != claim.engine_id()
            || handoff.docker_root_digest() != claim.docker_root_digest()
            || handoff.metadata_volume_name()
                != claim.metadata_volume_name().ok_or(HostError::Identity)?
            || handoff.metadata_volume_id()
                != claim.metadata_volume_id().ok_or(HostError::Identity)?
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            final_readiness,
            observed,
        })
    }

    pub(crate) fn final_readiness(&self) -> &VerifiedMetadataInitialization {
        &self.final_readiness
    }

    pub(crate) fn connection_id(&self) -> &str {
        self.final_readiness.connection_id()
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.final_readiness.reservation_id()
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.final_readiness.operation_id()
    }
    pub(crate) fn engine_id(&self) -> &str {
        self.final_readiness.engine_id()
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        self.final_readiness.docker_root_digest()
    }
    pub(crate) fn metadata_volume_name(&self) -> &str {
        self.final_readiness.metadata_volume_name()
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        self.final_readiness.metadata_volume_id()
    }
    pub(crate) const fn layout_version(&self) -> i64 {
        self.final_readiness.layout_version()
    }
    pub(crate) fn layout_digest(&self) -> &str {
        self.final_readiness.layout_digest()
    }
    pub(crate) fn job_facts(&self) -> &super::request_result::ObservedDirectory {
        self.observed.job_facts()
    }

    pub(crate) fn metadata_root(&self) -> &super::request_result::ObservedDirectory {
        self.observed.metadata()
    }

    pub(crate) fn runtime_identity(&self) -> &super::request_result::ObservedDirectory {
        self.observed.runtime_identity()
    }
}
