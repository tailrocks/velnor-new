//! This module returns a cleanup proof only after Docker and guest checks make sure that the job resources are gone.

use crate::{
    error::HostError,
    journal::{StorageCleanupIdentity, StorageCleanupObserverWitness, StorageRole},
};

use super::{
    VerifiedStorageCleanup, production::ObservedDockerCleanupAbsence,
    validation::validate_observations,
};
use crate::launch::resource_probe::VerifiedCleanupObserverRecord;

pub(super) fn verified_storage_cleanup(
    identity: &StorageCleanupIdentity,
    witness: &StorageCleanupObserverWitness,
    docker: &ObservedDockerCleanupAbsence,
    observer: &VerifiedCleanupObserverRecord,
) -> Result<VerifiedStorageCleanup, HostError> {
    validate_observations(identity, witness, docker, observer)?;
    if identity.reservation_id() != docker.reservation_id()
        || identity.operation_id() != docker.operation_id()
        || identity.engine_id() != docker.engine_id()
        || identity.docker_root_digest() != docker.docker_root_digest()
        || identity.runner_container_id() != docker.runner_container_id()
        || identity.dind_container_id() != docker.dind_container_id()
        || identity.instance_binding_digest() != witness.instance_binding_digest()
        || identity.volumes() != docker.volume_inventory()
    {
        return Err(HostError::Ownership);
    }
    let role_inventory = StorageRole::PHYSICAL.to_vec();
    let volume_names = identity
        .volumes()
        .iter()
        .map(|(_, name)| name.clone())
        .collect();
    Ok(VerifiedStorageCleanup {
        reservation_id: identity.reservation_id(),
        operation_id: identity.operation_id(),
        engine_id: identity.engine_id().to_owned(),
        docker_root_digest: identity.docker_root_digest().to_owned(),
        runner_container_id: identity.runner_container_id().to_owned(),
        dind_container_id: identity.dind_container_id().to_owned(),
        volume_names,
        instance_binding_digest: identity.instance_binding_digest().to_owned(),
        role_inventory,
        containers_absent: true,
        volumes_absent: true,
        mounts_detached: true,
        cgroups_absent: true,
    })
}
