use crate::{
    error::HostError, journal::StorageInitializationClaim,
    worker::storage_initialization::ObservedStorageInitialization,
};

mod lifecycle;

/// Docker data from the operation that creates one specified helper.
pub(crate) struct StorageInitializerCreateResponse {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    container_id: String,
}

/// Docker data from the operation that creates one specified helper.
pub(crate) struct VerifiedStorageInitializerCreated {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    container_id: String,
}

/// Docker data from the operation that starts one specified helper.
pub(crate) struct VerifiedStorageInitializerStarted {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    container_id: String,
}

/// Docker data that shows the specified helper ID is absent.
pub(crate) struct VerifiedStorageInitializerRemoved {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    container_id: String,
}

/// Docker data that shows no container has the helper name recorded in the journal.
pub(crate) struct VerifiedStorageInitializerAbsent {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    abort_claim_generation: i64,
}

/// Docker data from inspecting one owned helper before removal.
pub(crate) struct VerifiedStorageInitializerAbortRemoval {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    container_id: String,
    abort_claim_generation: i64,
}

/// Validated layout-v2 result from one exact initializer operation.
pub(crate) struct VerifiedStorageInitializerResult {
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    layout_version: i64,
    metadata_volume_name: String,
    metadata_volume_id: String,
    layout_digest: String,
    ready_digest: String,
}

/// Validated initializer READY result while the exact helper is still running.
pub(crate) struct VerifiedStorageInitializerAwaitingHandoff {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    generation: i64,
    operation_key: String,
    engine_id: String,
    docker_root_digest: String,
    helper_name: String,
    container_id: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    layout_version: i64,
    layout_digest: String,
    ready_digest: String,
}

impl VerifiedStorageInitializerAwaitingHandoff {
    pub(super) fn after_ready(
        connection_id: &str,
        claim: &StorageInitializationClaim,
        started: &VerifiedStorageInitializerStarted,
        result: &VerifiedStorageInitializerResult,
    ) -> Result<Self, HostError> {
        if connection_id.is_empty()
            || claim.reservation_id() != started.reservation_id()
            || claim.operation_id() != started.operation_id()
            || claim.generation() != started.generation()
            || claim.operation_key() != started.operation_key()
            || claim.engine_id() != started.engine_id()
            || claim.docker_root_digest() != started.docker_root_digest()
            || claim.helper_name() != started.helper_name()
            || Some(started.container_id()) != claim.helper_id()
            || result.reservation_id() != claim.reservation_id()
            || result.operation_id() != claim.operation_id()
            || result.generation() != claim.generation()
            || result.operation_key() != claim.operation_key()
            || result.engine_id() != claim.engine_id()
            || result.docker_root_digest() != claim.docker_root_digest()
            || result.layout_version() != 2
            || claim.layout_version() != 2
        {
            return Err(HostError::Identity);
        }
        let metadata_volume_name = claim
            .metadata_volume_name()
            .ok_or(HostError::Identity)?
            .to_owned();
        let metadata_volume_id = claim
            .metadata_volume_id()
            .ok_or(HostError::Identity)?
            .to_owned();
        if result.metadata_volume_name() != metadata_volume_name
            || result.metadata_volume_id() != metadata_volume_id
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            connection_id: connection_id.to_owned(),
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            container_id: started.container_id().to_owned(),
            metadata_volume_name,
            metadata_volume_id,
            layout_version: 2,
            layout_digest: result.layout_digest().to_owned(),
            ready_digest: result.ready_digest().to_owned(),
        })
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
    pub(crate) fn operation_key(&self) -> &str {
        &self.operation_key
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) fn helper_name(&self) -> &str {
        &self.helper_name
    }
    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }
    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }
    pub(crate) const fn layout_version(&self) -> i64 {
        self.layout_version
    }
    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }
    pub(crate) fn ready_digest(&self) -> &str {
        &self.ready_digest
    }
}

#[cfg(test)]
impl VerifiedStorageInitializerAwaitingHandoff {
    pub(crate) fn test_after_ready(
        connection_id: &str,
        claim: &StorageInitializationClaim,
        started: &VerifiedStorageInitializerStarted,
        result: &VerifiedStorageInitializerResult,
    ) -> Result<Self, HostError> {
        Self::after_ready(connection_id, claim, started, result)
    }
}

impl VerifiedStorageInitializerResult {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
    pub(crate) fn operation_key(&self) -> &str {
        &self.operation_key
    }
    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }
    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }
    pub(crate) const fn layout_version(&self) -> i64 {
        self.layout_version
    }
    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }
    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }
    pub(crate) fn ready_digest(&self) -> &str {
        &self.ready_digest
    }

    pub(super) fn after_validated_result(
        claim: &StorageInitializationClaim,
        result: &ObservedStorageInitialization,
    ) -> Result<Self, HostError> {
        if claim.layout_version() != 2 || result.operation_id() != claim.operation_key() {
            return Err(HostError::Identity);
        }
        let metadata_volume_name = claim
            .metadata_volume_name()
            .ok_or(HostError::Identity)?
            .to_owned();
        let metadata_volume_id = claim
            .metadata_volume_id()
            .ok_or(HostError::Identity)?
            .to_owned();
        Ok(Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            layout_version: 2,
            metadata_volume_name,
            metadata_volume_id,
            layout_digest: result.layout_digest().to_owned(),
            ready_digest: result.ready_digest().to_owned(),
        })
    }

    #[cfg(test)]
    pub(crate) fn test_after_validated_result(
        claim: &StorageInitializationClaim,
        result: &ObservedStorageInitialization,
    ) -> Result<Self, HostError> {
        Self::after_validated_result(claim, result)
    }
}
