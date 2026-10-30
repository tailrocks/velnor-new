use crate::journal::StorageRole;

/// Proof that every reservation-owned container and named tmpfs volume is
/// gone on the exact engine. Shared EngineMetadata itself is never claimed gone.
pub(crate) struct VerifiedStorageCleanup {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    volume_names: [String; 4],
    instance_binding_digest: String,
    role_inventory: Vec<StorageRole>,
    containers_absent: bool,
    volumes_absent: bool,
    mounts_detached: bool,
    cgroups_absent: bool,
}

impl VerifiedStorageCleanup {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }

    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }

    pub(crate) fn volume_names(&self) -> &[String; 4] {
        &self.volume_names
    }

    pub(crate) fn instance_binding_digest(&self) -> &str {
        &self.instance_binding_digest
    }

    pub(crate) fn role_inventory(&self) -> &[StorageRole] {
        &self.role_inventory
    }

    pub(crate) const fn all_owned_refs_absent(&self) -> bool {
        self.containers_absent && self.volumes_absent && self.mounts_detached && self.cgroups_absent
    }
}

#[cfg(test)]
pub(crate) fn test_verified_storage_cleanup(
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    volume_names: [String; 4],
    instance_binding_digest: String,
    role_inventory: Vec<StorageRole>,
    containers_absent: bool,
    volumes_absent: bool,
    mounts_detached: bool,
    cgroups_absent: bool,
) -> Result<VerifiedStorageCleanup, crate::error::HostError> {
    use crate::launch::bootstrap::test_fixtures::{
        valid_container_id, valid_digest, valid_engine_id,
    };

    let physical_roles = [
        StorageRole::RunnerRoot,
        StorageRole::RunnerHome,
        StorageRole::RunnerWork,
        StorageRole::RunnerTmp,
        StorageRole::RunnerShm,
        StorageRole::SharedRun,
        StorageRole::DindRoot,
        StorageRole::DindData,
        StorageRole::DindTmp,
        StorageRole::DindShm,
        StorageRole::EngineMetadata,
    ];
    if reservation_id <= 0
        || operation_id <= 0
        || !valid_engine_id(&engine_id)
        || !valid_digest(&docker_root_digest)
        || !valid_container_id(&runner_container_id)
        || !valid_container_id(&dind_container_id)
        || runner_container_id == dind_container_id
        || !valid_digest(&instance_binding_digest)
        || role_inventory.len() != physical_roles.len()
        || physical_roles
            .iter()
            .any(|role| !role_inventory.contains(role))
        || !containers_absent
        || !volumes_absent
        || !mounts_detached
        || !cgroups_absent
    {
        return Err(crate::error::HostError::Journal);
    }
    let nonce = volume_names[0]
        .strip_prefix('w')
        .filter(|nonce| {
            nonce.len() == 32
                && nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or(crate::error::HostError::Journal)?;
    let names = crate::worker::WorkerIdentityNames::from_nonce(nonce)?;
    let expected_names = [
        names.volume_name(crate::worker::WorkerVolumeRole::Socket),
        names.volume_name(crate::worker::WorkerVolumeRole::Home),
        names.volume_name(crate::worker::WorkerVolumeRole::Work),
        names.volume_name(crate::worker::WorkerVolumeRole::DindData),
    ];
    if volume_names != expected_names {
        return Err(crate::error::HostError::Journal);
    }
    Ok(VerifiedStorageCleanup {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        runner_container_id,
        dind_container_id,
        volume_names,
        instance_binding_digest,
        role_inventory,
        containers_absent,
        volumes_absent,
        mounts_detached,
        cgroups_absent,
    })
}
