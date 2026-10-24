use super::{
    BootstrapContainerRole, BootstrapStartReceipt, VerifiedBootstrapContainerCreated,
    VerifiedBootstrapMountHandoff, VerifiedCreatedStorageObject,
};

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
#[expect(
    clippy::too_many_arguments,
    reason = "the test constructor keeps each bootstrap identity and container fact explicit for receipt validation"
)]
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
#[expect(
    clippy::too_many_arguments,
    reason = "the test constructor keeps the journal identity and both startup-gate facts explicit for receipt validation"
)]
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
