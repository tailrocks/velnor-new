use super::{
    StorageInitializationClaim, StorageInitializerCreateResponse,
    VerifiedStorageInitializerAbortRemoval, VerifiedStorageInitializerAbsent,
    VerifiedStorageInitializerCreated, VerifiedStorageInitializerRemoved,
    VerifiedStorageInitializerStarted,
};

macro_rules! getters {
    ($name:ident) => {
        impl $name {
            pub(crate) fn reservation_id(&self) -> i64 {
                self.reservation_id
            }
            pub(crate) fn operation_id(&self) -> i64 {
                self.operation_id
            }
            pub(crate) fn generation(&self) -> i64 {
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
        }
    };
}

getters!(VerifiedStorageInitializerCreated);
getters!(VerifiedStorageInitializerStarted);
getters!(VerifiedStorageInitializerRemoved);
getters!(StorageInitializerCreateResponse);
getters!(VerifiedStorageInitializerAbortRemoval);

impl VerifiedStorageInitializerAbsent {
    pub(crate) fn reservation_id(&self) -> i64 {
        self.reservation_id
    }
    pub(crate) fn operation_id(&self) -> i64 {
        self.operation_id
    }
    pub(crate) fn generation(&self) -> i64 {
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
    pub(crate) fn abort_claim_generation(&self) -> i64 {
        self.abort_claim_generation
    }

    pub(in crate::worker::storage_initialization) fn after_exact_name_404(
        claim: &StorageInitializationClaim,
    ) -> Self {
        Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            abort_claim_generation: claim.abort_claim_generation(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_absent(claim: &StorageInitializationClaim) -> Self {
        Self::after_exact_name_404(claim)
    }
}

impl VerifiedStorageInitializerAbortRemoval {
    pub(crate) fn abort_claim_generation(&self) -> i64 {
        self.abort_claim_generation
    }

    pub(in crate::worker::storage_initialization) fn after_owned_inspection(
        claim: &StorageInitializationClaim,
        container_id: &str,
    ) -> Self {
        Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            container_id: container_id.to_owned(),
            abort_claim_generation: claim.abort_claim_generation(),
        }
    }
}

impl StorageInitializerCreateResponse {
    pub(in crate::worker::storage_initialization) fn after_create_response(
        claim: &StorageInitializationClaim,
        id: &str,
    ) -> Self {
        Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            container_id: id.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_response(claim: &StorageInitializationClaim, id: &str) -> Self {
        Self::after_create_response(claim, id)
    }
}

impl VerifiedStorageInitializerCreated {
    pub(in crate::worker::storage_initialization) fn from_inspection(
        claim: &StorageInitializationClaim,
        id: &str,
    ) -> Self {
        Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            container_id: id.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_created(claim: &StorageInitializationClaim, id: &str) -> Self {
        Self::from_inspection(claim, id)
    }

    #[cfg(test)]
    pub(crate) fn test_created_with_engine(
        claim: &StorageInitializationClaim,
        id: &str,
        engine_id: &str,
    ) -> Self {
        let mut proof = Self::from_inspection(claim, id);
        proof.engine_id = engine_id.to_owned();
        proof
    }
}

impl VerifiedStorageInitializerStarted {
    pub(in crate::worker::storage_initialization) fn from_inspection(
        claim: &StorageInitializationClaim,
        id: &str,
    ) -> Self {
        Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            container_id: id.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_started(claim: &StorageInitializationClaim, id: &str) -> Self {
        Self::from_inspection(claim, id)
    }
}

impl VerifiedStorageInitializerRemoved {
    pub(in crate::worker::storage_initialization) fn after_absence(
        claim: &StorageInitializationClaim,
        id: &str,
    ) -> Self {
        Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            generation: claim.generation(),
            operation_key: claim.operation_key().to_owned(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            helper_name: claim.helper_name().to_owned(),
            container_id: id.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_removed(claim: &StorageInitializationClaim, id: &str) -> Self {
        Self::after_absence(claim, id)
    }
}
