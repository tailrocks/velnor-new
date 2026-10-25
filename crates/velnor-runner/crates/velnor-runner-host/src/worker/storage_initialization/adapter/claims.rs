use crate::{
    error::HostError,
    journal::StorageInitializationClaim,
    worker::storage_initialization::{
        StorageInitializationImage,
        proof::{VerifiedStorageInitializerCreated, VerifiedStorageInitializerStarted},
    },
};

pub(super) fn verify_claim_image(
    claim: &StorageInitializationClaim,
    image: &StorageInitializationImage,
) -> Result<(), HostError> {
    if claim.image_runtime_id() != image.runtime_id()
        || claim.image_source_sha() != image.source_sha()
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

pub(super) fn same_create_claim(
    claim: &StorageInitializationClaim,
    proof: &VerifiedStorageInitializerCreated,
) -> bool {
    proof.reservation_id() == claim.reservation_id()
        && proof.operation_id() == claim.operation_id()
        && proof.generation() == claim.generation()
        && proof.operation_key() == claim.operation_key()
        && proof.engine_id() == claim.engine_id()
        && proof.docker_root_digest() == claim.docker_root_digest()
        && proof.helper_name() == claim.helper_name()
}

pub(crate) fn same_start_claim(
    claim: &StorageInitializationClaim,
    proof: &VerifiedStorageInitializerStarted,
) -> bool {
    proof.reservation_id() == claim.reservation_id()
        && proof.operation_id() == claim.operation_id()
        && proof.generation() == claim.generation()
        && proof.operation_key() == claim.operation_key()
        && proof.engine_id() == claim.engine_id()
        && proof.docker_root_digest() == claim.docker_root_digest()
        && proof.helper_name() == claim.helper_name()
}
