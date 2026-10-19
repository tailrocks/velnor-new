mod network;
mod pair;
mod storage_object;
#[cfg(test)]
pub(crate) use network::remove_owned_network;
#[cfg(test)]
pub(crate) use pair::PairStartFailure;

pub(crate) use network::{
    VerifiedStorageNetworkCreated, VerifiedStorageNetworkRemoved, create_owned_network,
};
pub(crate) use pair::{PendingBootstrapPair, StartedBootstrapPair, start_bootstrap_pair};
pub(crate) use storage_object::{create_metadata_storage_object, create_storage_object};

/// Proof that both workers mounted the exact initialized storage objects.
pub(crate) struct VerifiedBootstrapMountHandoff {
    connection_id: String,
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    metadata_volume_name: String,
    metadata_volume_id: String,
    layout_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    mount_evidence_digest: String,
}

impl VerifiedBootstrapMountHandoff {
    pub(crate) fn connection_id(&self) -> &str {
        &self.connection_id
    }
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
    pub(crate) fn metadata_volume_name(&self) -> &str {
        &self.metadata_volume_name
    }
    pub(crate) fn metadata_volume_id(&self) -> &str {
        &self.metadata_volume_id
    }
    pub(crate) fn layout_digest(&self) -> &str {
        &self.layout_digest
    }
    pub(crate) fn runner_container_id(&self) -> &str {
        &self.runner_container_id
    }
    pub(crate) fn dind_container_id(&self) -> &str {
        &self.dind_container_id
    }
    pub(crate) fn mount_evidence_digest(&self) -> &str {
        &self.mount_evidence_digest
    }
}

#[cfg(test)]
pub(crate) fn test_bootstrap_mount_handoff(
    metadata: &crate::journal::VerifiedMetadataInitializationHandoff,
    started: &BootstrapStartReceipt,
    mount_evidence_digest: &str,
) -> Result<VerifiedBootstrapMountHandoff, crate::error::HostError> {
    if metadata.reservation_id() != started.reservation_id()
        || metadata.operation_id() != started.operation_id()
        || metadata.engine_id() != started.engine_id()
        || metadata.docker_root_digest() != started.docker_root_digest()
        || started.runner_container_id() == started.dind_container_id()
        || mount_evidence_digest.len() != 64
        || !mount_evidence_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(crate::error::HostError::Identity);
    }
    Ok(VerifiedBootstrapMountHandoff {
        connection_id: metadata.connection_id().to_owned(),
        reservation_id: metadata.reservation_id(),
        operation_id: metadata.operation_id(),
        engine_id: metadata.engine_id().to_owned(),
        docker_root_digest: metadata.docker_root_digest().to_owned(),
        metadata_volume_name: metadata.metadata_volume_name().to_owned(),
        metadata_volume_id: metadata.metadata_volume_id().to_owned(),
        layout_digest: metadata.layout_digest().to_owned(),
        runner_container_id: started.runner_container_id().to_owned(),
        dind_container_id: started.dind_container_id().to_owned(),
        mount_evidence_digest: mount_evidence_digest.to_owned(),
    })
}

/// One of the two fixed bootstrap containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BootstrapContainerRole {
    /// Unprivileged Actions runner, started with stdin open and no JIT bytes.
    Runner,
    /// Private privileged `DinD` service for this runner only.
    Dind,
}

/// Exact Docker create response and inspect evidence for one bootstrap container.
pub(crate) struct VerifiedBootstrapContainerCreated {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: BootstrapContainerRole,
    container_name: String,
    container_id: String,
}

impl VerifiedBootstrapContainerCreated {
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

    pub(crate) const fn role(&self) -> BootstrapContainerRole {
        self.role
    }

    pub(crate) fn container_name(&self) -> &str {
        &self.container_name
    }

    pub(crate) fn container_id(&self) -> &str {
        &self.container_id
    }
}

/// Exact returned and inspected identity of one named tmpfs volume object.
pub(crate) struct VerifiedCreatedStorageObject {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: crate::journal::StorageRole,
    docker_name: String,
    docker_id: String,
}

impl VerifiedCreatedStorageObject {
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

    pub(crate) const fn role(&self) -> crate::journal::StorageRole {
        self.role
    }

    pub(crate) fn docker_name(&self) -> &str {
        &self.docker_name
    }

    /// Docker volumes have no separate permanent ID. This value is the inspected volume name.
    pub(crate) fn docker_id(&self) -> &str {
        &self.docker_id
    }
}

/// The pair is running while the official Runner listener remains blocked on
/// its open stdin. This receipt never includes JIT bytes.
pub(crate) struct BootstrapStartReceipt {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    runner_stdin_open: bool,
    jit_withheld: bool,
}

impl BootstrapStartReceipt {
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

    pub(crate) const fn runner_stdin_open(&self) -> bool {
        self.runner_stdin_open
    }

    pub(crate) const fn jit_withheld(&self) -> bool {
        self.jit_withheld
    }
}

#[cfg(test)]
pub(crate) fn test_created_storage_object(
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    role: crate::journal::StorageRole,
    docker_name: String,
    docker_id: String,
) -> Result<VerifiedCreatedStorageObject, crate::error::HostError> {
    use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

    let suffix_role = match role {
        crate::journal::StorageRole::SharedRun => ("", WorkerVolumeRole::Socket),
        crate::journal::StorageRole::RunnerHome => ("-home", WorkerVolumeRole::Home),
        crate::journal::StorageRole::RunnerWork => ("-work", WorkerVolumeRole::Work),
        crate::journal::StorageRole::DindData => ("-docker", WorkerVolumeRole::DindData),
        crate::journal::StorageRole::EngineMetadata => {
            ("-metadata", WorkerVolumeRole::RunnerMetadata)
        }
        _ => return Err(crate::error::HostError::Journal),
    };
    let base = docker_name
        .strip_suffix(suffix_role.0)
        .ok_or(crate::error::HostError::Journal)?;
    let nonce = base
        .strip_prefix('w')
        .filter(|nonce| {
            nonce.len() == 32
                && nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or(crate::error::HostError::Journal)?;
    let names = WorkerIdentityNames::from_nonce(nonce)?;
    if reservation_id <= 0
        || operation_id <= 0
        || !valid_engine_id(&engine_id)
        || !valid_digest(&docker_root_digest)
        || names.volume_name(suffix_role.1) != docker_name
        || docker_id != docker_name
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(VerifiedCreatedStorageObject {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        role,
        docker_name,
        docker_id,
    })
}

#[cfg(test)]
pub(crate) fn test_created_bootstrap_container(
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    nonce: &str,
    role: BootstrapContainerRole,
    container_name: String,
    container_id: String,
) -> Result<VerifiedBootstrapContainerCreated, crate::error::HostError> {
    use crate::worker::{WorkerContainerRole, WorkerIdentityNames};

    let names = WorkerIdentityNames::from_nonce(nonce)?;
    let expected = match role {
        BootstrapContainerRole::Runner => WorkerContainerRole::Runner,
        BootstrapContainerRole::Dind => WorkerContainerRole::Dind,
    };
    if reservation_id <= 0
        || operation_id <= 0
        || !valid_engine_id(&engine_id)
        || !valid_digest(&docker_root_digest)
        || !valid_container_id(&container_id)
        || container_name != names.container_name(expected)
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(VerifiedBootstrapContainerCreated {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        role,
        container_name,
        container_id,
    })
}

#[cfg(test)]
pub(crate) fn test_bootstrap_start_receipt(
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    runner_stdin_open: bool,
    jit_withheld: bool,
) -> Result<BootstrapStartReceipt, crate::error::HostError> {
    if reservation_id <= 0
        || operation_id <= 0
        || !valid_engine_id(&engine_id)
        || !valid_digest(&docker_root_digest)
        || !valid_container_id(&runner_container_id)
        || !valid_container_id(&dind_container_id)
        || runner_container_id == dind_container_id
        || !runner_stdin_open
        || !jit_withheld
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(BootstrapStartReceipt {
        reservation_id,
        operation_id,
        engine_id,
        docker_root_digest,
        runner_container_id,
        dind_container_id,
        runner_stdin_open,
        jit_withheld,
    })
}

#[cfg(test)]
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
fn valid_engine_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

#[cfg(test)]
fn valid_container_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
