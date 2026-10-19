use std::future::Future;
use std::time::Instant;

use bollard::Docker;
use bollard::errors::Error as DockerError;

use super::{PairCreate, REQUEST_TIMEOUT};
use crate::docker_client::docker_deadline_after;
use crate::error::HostError;

pub(super) async fn inspect(
    docker: &Docker,
    id: &str,
    deadline: Instant,
) -> Result<Option<bollard::models::ContainerInspectResponse>, HostError> {
    match request(deadline, docker.inspect_container(id, None)).await? {
        Ok(container) => Ok(Some(container)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(super) fn verify_created(
    inspected: &bollard::models::ContainerInspectResponse,
    expected_id: &str,
    claim: &crate::journal::BootstrapContainerCreateClaim,
    spec: &PairCreate,
    image: &bollard::models::ImageConfig,
) -> Result<(), HostError> {
    let name = format!("/{}", claim.container_name());
    let Some(config) = inspected.config.as_ref() else {
        return Err(HostError::Ownership);
    };
    let Some(host) = inspected.host_config.as_ref() else {
        return Err(HostError::Ownership);
    };
    let expected_host = spec
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Identity)?;
    if !effective_config_matches(config, image, &spec.config)? {
        return Err(HostError::Ownership);
    }
    if inspected.id.as_deref() != Some(expected_id)
        || inspected.name.as_deref() != Some(name.as_str())
        || inspected.image.as_deref() != config.image.as_deref()
        || config.image != spec.config.image
        || host.privileged != expected_host.privileged
        || host.readonly_rootfs != expected_host.readonly_rootfs
        || host.pids_limit != expected_host.pids_limit
        || host.nano_cpus != expected_host.nano_cpus
        || host.memory != expected_host.memory
        || host.memory_swap != expected_host.memory_swap
        || host.network_mode != expected_host.network_mode
        || host.tmpfs != expected_host.tmpfs
        || host.mounts != expected_host.mounts
        || host.log_config != expected_host.log_config
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

fn effective_config_matches(
    actual: &bollard::models::ContainerConfig,
    image: &bollard::models::ImageConfig,
    request: &bollard::models::ContainerCreateBody,
) -> Result<bool, HostError> {
    let expected_labels = merge_labels(image.labels.as_ref(), request.labels.as_ref());
    let expected_env = merge_environment(image.env.as_deref(), request.env.as_deref())?;
    let expected_cmd = request.cmd.as_ref().or(image.cmd.as_ref());
    let expected_entrypoint = request.entrypoint.as_ref().or(image.entrypoint.as_ref());
    let expected_user = request.user.as_ref().or(image.user.as_ref());
    let expected_working_dir = request.working_dir.as_ref().or(image.working_dir.as_ref());
    Ok(actual.labels.clone().unwrap_or_default() == expected_labels
        && merge_environment(actual.env.as_deref(), None)? == expected_env
        && actual.cmd.as_ref() == expected_cmd
        && actual.entrypoint.as_ref() == expected_entrypoint
        && actual.user.as_ref() == expected_user
        && actual.working_dir.as_ref() == expected_working_dir
        && actual.stdin_once == request.stdin_once
        && actual.open_stdin == request.open_stdin)
}

fn merge_labels(
    image: Option<&std::collections::HashMap<String, String>>,
    request: Option<&std::collections::HashMap<String, String>>,
) -> std::collections::HashMap<String, String> {
    let mut merged = image.cloned().unwrap_or_default();
    if let Some(request) = request {
        merged.extend(request.clone());
    }
    merged
}

fn merge_environment(
    image: Option<&[String]>,
    request: Option<&[String]>,
) -> Result<std::collections::BTreeMap<String, String>, HostError> {
    let mut merged = parse_environment(image)?;
    merged.extend(parse_environment(request)?);
    Ok(merged)
}

fn parse_environment(
    entries: Option<&[String]>,
) -> Result<std::collections::BTreeMap<String, String>, HostError> {
    let mut parsed = std::collections::BTreeMap::new();
    for entry in entries.into_iter().flatten() {
        let (name, value) = entry.split_once('=').ok_or(HostError::Identity)?;
        if name.is_empty() {
            return Err(HostError::Identity);
        }
        if parsed.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(HostError::Identity);
        }
    }
    Ok(parsed)
}

pub(super) async fn request<F: Future>(
    deadline: Instant,
    future: F,
) -> Result<F::Output, HostError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(HostError::Docker)?;
    docker_deadline_after(future, remaining.min(REQUEST_TIMEOUT)).await
}

pub(super) fn valid_resource_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests;
