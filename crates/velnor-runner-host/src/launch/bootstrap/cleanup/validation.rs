// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    error::HostError,
    journal::{StorageCleanupIdentity, StorageCleanupObserverWitness},
};

use super::production::ObservedDockerCleanupAbsence;
use crate::launch::resource_probe::VerifiedCleanupObserverRecord;

pub(super) fn validate_observations(
    identity: &StorageCleanupIdentity,
    witness: &StorageCleanupObserverWitness,
    docker: &ObservedDockerCleanupAbsence,
    observer: &VerifiedCleanupObserverRecord,
) -> Result<(), HostError> {
    let binding = witness.binding();
    let record = observer.record();
    if observer.reservation_id() != witness.reservation_id()
        || observer.engine_id() != witness.engine_id()
        || observer.docker_root_digest() != witness.docker_root_digest()
        || record.operation_id != observer.operation_id()
        || record.binding_sha256 != witness.binding_sha256()
        || docker.reservation_id() != witness.reservation_id()
        || docker.operation_id() != witness.operation_id()
        || docker.engine_id() != witness.engine_id()
        || docker.docker_root_digest() != witness.docker_root_digest()
        || docker.runner_container_id() != witness.runner_container_id()
        || docker.dind_container_id() != witness.dind_container_id()
        || docker.volume_inventory() != identity.volumes()
        || binding
            .volume_mountpoints
            .iter()
            .map(|volume| volume.role.as_str())
            .ne(identity.volumes().iter().map(|(role, _)| role.as_str()))
    {
        return Err(HostError::Ownership);
    }
    velnor_resource_probe::cleanup_observer_wire::validate_cleanup_observer_record(
        record,
        binding,
        observer.operation_id(),
        witness.binding_sha256(),
    )
    .map_err(|_| HostError::Ownership)
}
