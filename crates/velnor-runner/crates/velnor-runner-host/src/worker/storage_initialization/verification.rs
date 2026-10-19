use bollard::{
    Docker,
    errors::Error as DockerError,
    models::{ContainerInspectResponse, MountType},
};

use crate::{
    docker_client::docker_deadline,
    error::HostError,
    journal::{ConnectionJournal, StorageInitializationClaim},
    launch::SelectedEngineContext,
    worker::{WorkerIdentityNames, WorkerVolumeRole},
};

const IMAGE_PATH_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

pub(super) async fn verify_engine(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
) -> Result<(), HostError> {
    let selected = journal
        .engine_context()
        .await?
        .ok_or(HostError::Ownership)?;
    let info = docker_deadline(docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let actual = SelectedEngineContext::capture(&selected, &info)?;
    if !engine_identity_matches(
        actual.engine_id(),
        actual.docker_root_digest(),
        claim.engine_id(),
        claim.docker_root_digest(),
    ) {
        return Err(HostError::Ownership);
    }
    Ok(())
}

pub(super) async fn verify_image(
    docker: &Docker,
    claim: &StorageInitializationClaim,
) -> Result<Vec<String>, HostError> {
    let image = docker_deadline(docker.inspect_image(claim.image_runtime_id()))
        .await?
        .map_err(|_| HostError::Docker)?;
    if image.id.as_deref() != Some(claim.image_runtime_id())
        || image.os.as_deref() != Some("linux")
        || image.architecture.as_deref() != Some("amd64")
    {
        return Err(HostError::Ownership);
    }
    let env = image
        .config
        .as_ref()
        .and_then(|config| config.env.as_ref())
        .ok_or(HostError::Ownership)?;
    if !image_environment_matches(env) {
        return Err(HostError::Ownership);
    }
    Ok(env.clone())
}

pub(super) async fn verify_volumes(
    docker: &Docker,
    claim: &StorageInitializationClaim,
) -> Result<(), HostError> {
    if claim.layout_version() != 2 {
        return Err(HostError::Ownership);
    }
    let metadata_name = claim.metadata_volume_name().ok_or(HostError::Ownership)?;
    let metadata_id = claim.metadata_volume_id().ok_or(HostError::Ownership)?;
    let base = claim
        .home_volume()
        .strip_suffix("-home")
        .ok_or(HostError::Ownership)?;
    let names = WorkerIdentityNames::from_private_volume(base)?;
    if names.volume_name(WorkerVolumeRole::Home) != claim.home_volume()
        || names.volume_name(WorkerVolumeRole::Work) != claim.work_volume()
        || names.volume_name(WorkerVolumeRole::RunnerMetadata) != metadata_name
    {
        return Err(HostError::Ownership);
    }
    for (name, role, expected_id) in [
        (claim.home_volume(), "runner-home", None),
        (claim.work_volume(), "runner-work", None),
        (metadata_name, "engine-metadata", Some(metadata_id)),
    ] {
        let volume = docker_deadline(docker.inspect_volume(name))
            .await?
            .map_err(|error| match error {
                DockerError::DockerResponseServerError {
                    status_code: 404, ..
                } => HostError::Ownership,
                _ => HostError::Docker,
            })?;
        let labels = volume.labels;
        if !volume_identity_matches(
            &volume.name,
            &labels,
            name,
            claim.connection_id(),
            claim.reservation_id(),
            claim.operation_id(),
            role,
        ) || expected_id.is_some_and(|expected| volume.name != expected)
        {
            return Err(HostError::Ownership);
        }
    }
    Ok(())
}

pub(super) async fn inspect_container(
    docker: &Docker,
    id: &str,
) -> Result<Option<ContainerInspectResponse>, HostError> {
    match docker_deadline(docker.inspect_container(id, None)).await? {
        Ok(container) => Ok(Some(container)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) async fn inspect_owned_container(
    docker: &Docker,
    claim: &StorageInitializationClaim,
    id: &str,
) -> Result<ContainerInspectResponse, HostError> {
    let inspected = inspect_container(docker, id)
        .await?
        .ok_or(HostError::Ownership)?;
    if !container_matches_claim_identity(&inspected, claim, id) {
        return Err(HostError::Ownership);
    }
    Ok(inspected)
}

pub(super) fn container_matches_claim(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
    expected_env: &[String],
) -> bool {
    container_matches_claim_identity(
        inspected,
        claim,
        inspected.id.as_deref().unwrap_or_default(),
    ) && inspected.image.as_deref() == Some(claim.image_runtime_id())
        && initializer_config_matches(inspected, claim, expected_env)
        && initializer_host_config_matches(inspected, claim)
        && initializer_mounts_match(inspected, claim)
}

fn initializer_config_matches(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
    expected_env: &[String],
) -> bool {
    inspected.config.as_ref().is_some_and(|config| {
        config.image.as_deref() == Some(claim.image_runtime_id())
            && config.user.as_deref() == Some("0:0")
            && config.entrypoint.as_deref() == Some(["/velnor/storage-init".to_owned()].as_slice())
            && config.attach_stdin == Some(true)
            && config.attach_stdout == Some(true)
            && config.attach_stderr == Some(true)
            && config.open_stdin == Some(true)
            && config.stdin_once == Some(true)
            && config.tty == Some(false)
            && config.env.as_deref() == Some(expected_env)
            && config.cmd.as_ref().is_none_or(Vec::is_empty)
    })
}

fn initializer_host_config_matches(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
) -> bool {
    let Some(expected_mounts) = expected_initializer_mounts(claim) else {
        return false;
    };
    inspected.host_config.as_ref().is_some_and(|host| {
        host.nano_cpus == Some(claim.nano_cpus())
            && host.memory == Some(claim.memory_bytes())
            && host.memory_swap == Some(claim.memory_bytes())
            && host.pids_limit == Some(claim.pids_limit())
            && host.network_mode.as_deref() == Some("none")
            && host.privileged == Some(false)
            && host.readonly_rootfs == Some(true)
            && host.binds.as_ref().is_none_or(Vec::is_empty)
            && host.cap_drop.as_deref() == Some(["ALL".to_owned()].as_slice())
            && host
                .cap_add
                .as_deref()
                .is_some_and(|items| capabilities_match(items, &["CHOWN", "FOWNER"]))
            && host.security_opt.as_deref()
                == Some(["no-new-privileges:true".to_owned()].as_slice())
            && host.mounts.as_ref().is_some_and(|mounts| {
                mounts.len() == expected_mounts.len()
                    && expected_mounts.iter().all(|(source, target)| {
                        mounts.iter().any(|mount| {
                            mount.source.as_deref() == Some(*source)
                                && mount.target.as_deref() == Some(*target)
                                && mount.typ == Some(MountType::VOLUME)
                                && !mount.read_only.unwrap_or(false)
                                && mount
                                    .volume_options
                                    .as_ref()
                                    .is_some_and(|options| options.no_copy == Some(true))
                        })
                    })
            })
    })
}

fn initializer_mounts_match(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
) -> bool {
    let Some(expected_mounts) = expected_initializer_mounts(claim) else {
        return false;
    };
    inspected.mounts.as_ref().is_some_and(|mounts| {
        mounts.len() == expected_mounts.len()
            && expected_mounts.iter().all(|(name, destination)| {
                mounts.iter().any(|mount| {
                    mount.name.as_deref() == Some(*name)
                        && mount.destination.as_deref() == Some(*destination)
                        && mount.rw == Some(true)
                })
            })
    })
}

fn expected_initializer_mounts(claim: &StorageInitializationClaim) -> Option<Vec<(&str, &str)>> {
    let mut expected = vec![
        (claim.home_volume(), "/velnor/init/home"),
        (claim.work_volume(), "/velnor/init/work"),
    ];
    match claim.layout_version() {
        1 if claim.metadata_volume_name().is_none() && claim.metadata_volume_id().is_none() => {}
        2 => expected.push((claim.metadata_volume_name()?, "/velnor/init/metadata")),
        _ => return None,
    }
    Some(expected)
}

fn capabilities_match(actual: &[String], expected: &[&str]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    let mut normalized = Vec::with_capacity(actual.len());
    for capability in actual {
        let value = capability.strip_prefix("CAP_").unwrap_or(capability);
        if !expected.contains(&value) || normalized.contains(&value) {
            return false;
        }
        normalized.push(value);
    }
    expected
        .iter()
        .all(|capability| normalized.contains(capability))
}

fn image_environment_matches(actual: &[String]) -> bool {
    actual == [IMAGE_PATH_ENV]
}

pub(super) fn container_matches_claim_identity(
    inspected: &ContainerInspectResponse,
    claim: &StorageInitializationClaim,
    id: &str,
) -> bool {
    let labels = inspected
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref());
    inspected.id.as_deref() == Some(id)
        && inspected.name.as_deref() == Some(format!("/{}", claim.helper_name()).as_str())
        && network_isolated(inspected)
        && labels.is_some_and(|labels| {
            labels.get("velnor.role").map(String::as_str) == Some("storage-initializer")
                && labels.get("velnor.operation").map(String::as_str) == Some(claim.operation_key())
        })
}

fn network_isolated(inspected: &ContainerInspectResponse) -> bool {
    let Some(settings) = inspected.network_settings.as_ref() else {
        return false;
    };
    if !settings.sandbox_id.as_deref().is_some_and(str::is_empty)
        || !settings.sandbox_key.as_deref().is_some_and(str::is_empty)
        || settings
            .ports
            .as_ref()
            .is_none_or(|ports| !ports.is_empty())
    {
        return false;
    }
    let Some(networks) = settings.networks.as_ref() else {
        return false;
    };
    if networks.len() != 1 {
        return false;
    }
    let Some(none) = networks.get("none") else {
        return false;
    };
    none.ipam_config.is_none()
        && none.links.as_ref().is_none_or(Vec::is_empty)
        && none.mac_address.as_deref().is_some_and(str::is_empty)
        && none.aliases.as_ref().is_none_or(Vec::is_empty)
        && none
            .driver_opts
            .as_ref()
            .is_none_or(std::collections::HashMap::is_empty)
        && none.network_id.as_deref().is_some_and(str::is_empty)
        && none.endpoint_id.as_deref().is_some_and(str::is_empty)
        && none.gateway.as_deref().is_some_and(str::is_empty)
        && none.ip_address.as_deref().is_some_and(str::is_empty)
        && none.ip_prefix_len == Some(0)
        && none.ipv6_gateway.as_deref().is_some_and(str::is_empty)
        && none
            .global_ipv6_address
            .as_deref()
            .is_some_and(str::is_empty)
        && none.global_ipv6_prefix_len == Some(0)
        && none.dns_names.as_ref().is_none_or(Vec::is_empty)
}

fn engine_identity_matches(
    actual_id: &str,
    actual_root: &str,
    expected_id: &str,
    expected_root: &str,
) -> bool {
    !actual_id.is_empty()
        && !actual_root.is_empty()
        && actual_id == expected_id
        && actual_root == expected_root
}

fn volume_identity_matches(
    actual_name: &str,
    labels: &std::collections::HashMap<String, String>,
    expected_name: &str,
    connection_id: &str,
    reservation_id: i64,
    operation_id: i64,
    role: &str,
) -> bool {
    actual_name == expected_name
        && labels.get("velnor.connection").map(String::as_str) == Some(connection_id)
        && labels.get("velnor.reservation").map(String::as_str)
            == Some(reservation_id.to_string().as_str())
        && labels.get("velnor.operation").map(String::as_str)
            == Some(operation_id.to_string().as_str())
        && labels.get("velnor.role").map(String::as_str) == Some(role)
}

#[cfg(test)]
#[path = "verification/tests.rs"]
mod tests;
