// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod docker;
mod mutation_cleanup;
mod mutation_lifecycle;
mod probes;
mod resources;
mod source;

use std::future::Future;
use std::path::Path;
use std::time::{Duration, Instant};

use bollard::container::LogOutput;
use bollard::errors::Error as DockerError;
use bollard::models::{ContainerInspectResponse, Mount, MountType, Volume};
use bollard::query_parameters::{
    CreateContainerOptions, LogsOptionsBuilder, WaitContainerOptionsBuilder,
};
use futures_util::StreamExt;

use crate::error::HostError;

const QUALIFIER_LABEL: &str = "io.velnor.runner-image-qualifier.run";
const OUTPUT_LIMIT: usize = 4096;

pub(super) async fn deadline<F: Future>(
    future: F,
    limit: Duration,
) -> Result<F::Output, HostError> {
    tokio::time::timeout(limit, future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

pub(super) fn remaining(deadline: Instant) -> Result<Duration, HostError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        Err(HostError::DockerTimeout)
    } else {
        Ok(remaining)
    }
}

pub(super) async fn wait_success(
    docker: &bollard::Docker,
    id: &str,
    timeout: Duration,
) -> Result<(), HostError> {
    let options = WaitContainerOptionsBuilder::new()
        .condition("not-running")
        .build();
    let wait = async {
        let mut responses = docker.wait_container(id, Some(options));
        let response = responses
            .next()
            .await
            .ok_or(HostError::Docker)?
            .map_err(|_| HostError::Docker)?;
        if response.status_code == 0 && response.error.is_none() {
            Ok(())
        } else {
            Err(HostError::Docker)
        }
    };
    deadline(wait, timeout).await?
}

pub(super) async fn read_logs(
    docker: &bollard::Docker,
    id: &str,
    timeout: Duration,
) -> Result<Vec<u8>, HostError> {
    let options = LogsOptionsBuilder::new()
        .follow(false)
        .stdout(true)
        .stderr(true)
        .timestamps(false)
        .tail("all")
        .build();
    let read = async {
        let mut stream = docker.logs(id, Some(options));
        let mut output = Vec::with_capacity(512);
        while let Some(item) = stream.next().await {
            match item.map_err(|_| HostError::Docker)? {
                LogOutput::StdOut { message } => append(&mut output, &message)?,
                LogOutput::StdErr { message } if message.is_empty() => {}
                _ => return Err(HostError::Docker),
            }
        }
        Ok(output)
    };
    deadline(read, timeout).await?
}

pub(super) fn append(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), HostError> {
    if output.len().saturating_add(bytes.len()) > OUTPUT_LIMIT {
        return Err(HostError::Frame);
    }
    output.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn create_options(name: &str) -> CreateContainerOptions {
    CreateContainerOptions {
        name: Some(name.to_owned()),
        platform: "linux/amd64".to_owned(),
    }
}

pub(super) fn mount(volume: &str, target: &str) -> Mount {
    Mount {
        target: Some(target.to_owned()),
        source: Some(volume.to_owned()),
        typ: Some(MountType::VOLUME),
        read_only: Some(false),
        ..Default::default()
    }
}

pub(super) fn validate_host_config(
    inspect: &ContainerInspectResponse,
    role: &str,
    volumes: &[&str],
) -> Result<(), HostError> {
    let host = inspect.host_config.as_ref().ok_or(HostError::Ownership)?;
    if host.privileged != Some(role == "dind")
        || host.network_mode.as_deref() != Some("none")
        || host
            .port_bindings
            .as_ref()
            .is_some_and(|ports| !ports.is_empty())
    {
        return Err(HostError::Ownership);
    }
    let expected_targets = match (role, volumes.len()) {
        ("dind", 2) => &["/run", "/var/lib/docker"][..],
        ("runner-socket-query", 1) => &["/run"][..],
        (_, 0) => &[][..],
        _ => return Err(HostError::Ownership),
    };
    let mounts = host.mounts.as_deref().unwrap_or_default();
    if mounts.len() != volumes.len() {
        return Err(HostError::Ownership);
    }
    for ((mount, volume), target) in mounts.iter().zip(volumes).zip(expected_targets) {
        if mount.typ != Some(MountType::VOLUME)
            || mount.source.as_deref() != Some(*volume)
            || mount.target.as_deref() != Some(*target)
            || mount.read_only == Some(true)
        {
            return Err(HostError::Ownership);
        }
    }
    Ok(())
}

pub(super) fn volume_owned(volume: &Volume, name: &str, run_id: &str, role: &str) -> bool {
    volume.name == name
        && volume.labels.get(QUALIFIER_LABEL).map(String::as_str) == Some(run_id)
        && volume
            .labels
            .get("io.velnor.runner-image-qualifier.role")
            .map(String::as_str)
            == Some(role)
}

pub(super) fn valid_container_id(value: &str) -> Result<(), HostError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

pub(super) fn missing(error: &DockerError) -> bool {
    matches!(
        error,
        DockerError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

pub(super) async fn qualify(
    root: &Path,
    source_sha: &str,
    runner_id: &str,
    dind_id: &str,
    docker_socket: &str,
) -> Result<(), HostError> {
    if !source::is_lower_hex(source_sha, 40)
        || !is_digest_id(runner_id)
        || !is_digest_id(dind_id)
        || runner_id == dind_id
        || !valid_socket(docker_socket)
    {
        return Err(HostError::Identity);
    }
    let source = source::load(root, source_sha).await?;
    let docker = crate::docker_client::connect_unix(docker_socket)?;
    let result = docker::qualify(&docker, &source, runner_id, dind_id).await?;
    source::revalidate(&source).await?;
    println!(
        "runner_listener_version={} runner_docker_version={} dind_docker_version={} dind_daemon_version={}",
        result.versions.runner_listener,
        result.versions.runner_docker,
        result.versions.dind_docker,
        result.versions.dind_daemon,
    );
    println!(
        "outer_docker_version={} outer_docker_id={} inner_docker_id={}",
        result.outer.version, result.outer.id, result.inner_id
    );
    println!(
        "qualified_runner_image_id={runner_id} qualified_dind_image_id={dind_id} source_revision={source_sha}"
    );
    Ok(())
}

fn is_digest_id(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    source::is_lower_hex(hex, 64)
}

fn valid_socket(value: &str) -> bool {
    let path = value.strip_prefix("unix://").unwrap_or(value);
    path.starts_with('/') && path.len() > 1 && !path.chars().any(char::is_control)
}
