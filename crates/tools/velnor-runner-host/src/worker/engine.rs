//! Docker operations used to create, start, and inspect worker containers.

use ::bollard::Docker;
use ::bollard::models::HostConfigCgroupnsModeEnum;
use ::bollard::query_parameters::{AttachContainerOptionsBuilder, StartContainerOptions};
use tokio::io::AsyncWriteExt;

use crate::HostError;
use crate::docker_client::docker_deadline;

use super::{CreateProjection, bollard_create};

pub(crate) async fn worker_id_for_name(
    docker: &Docker,
    name: &str,
    volume: &str,
    role: &str,
) -> Result<Option<String>, HostError> {
    if name.is_empty() || volume.is_empty() || role.is_empty() {
        return Err(HostError::Docker);
    }
    match Box::pin(docker_deadline(docker.inspect_container(name, None))).await? {
        Ok(body) => {
            let id = body
                .id
                .filter(|id| !id.is_empty())
                .ok_or(HostError::Docker)?;
            let host_config = body.host_config.ok_or(HostError::Docker)?;
            if host_config.cgroupns_mode != Some(HostConfigCgroupnsModeEnum::PRIVATE) {
                return Err(HostError::Docker);
            }
            let labels = body
                .config
                .and_then(|config| config.labels)
                .ok_or(HostError::Docker)?;
            if labels.get("velnor.volume").map(String::as_str) != Some(volume)
                || labels.get("velnor.worker").map(String::as_str) != Some(volume)
                || labels.get("velnor.role").map(String::as_str) != Some(role)
            {
                return Err(HostError::Docker);
            }
            Ok(Some(id))
        }
        Err(::bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

pub(crate) async fn create_only(
    docker: &Docker,
    spec: &CreateProjection,
) -> Result<String, HostError> {
    let created = bollard_create(spec)?;
    let response = docker
        .create_container(Some(created.options), created.config)
        .await
        .map_err(|_| HostError::Docker)?;
    if response.id.is_empty() {
        Err(HostError::Docker)
    } else {
        Ok(response.id)
    }
}

pub(crate) async fn start_id(docker: &Docker, id: &str) -> Result<(), HostError> {
    docker
        .start_container(id, None::<StartContainerOptions>)
        .await
        .map_err(|_| HostError::Docker)
}

pub(crate) async fn deliver_jit(docker: &Docker, id: &str, jit: &[u8]) -> Result<(), HostError> {
    let options = AttachContainerOptionsBuilder::new()
        .stdin(true)
        .stream(true)
        .build();
    let mut attached = docker
        .attach_container(id, Some(options))
        .await
        .map_err(|_| HostError::Docker)?;
    attached
        .input
        .write_all(jit)
        .await
        .map_err(|_| HostError::Docker)?;
    attached
        .input
        .shutdown()
        .await
        .map_err(|_| HostError::Docker)?;
    Ok(())
}
