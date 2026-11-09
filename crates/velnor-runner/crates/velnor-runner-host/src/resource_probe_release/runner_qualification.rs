// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod archive_operation;
mod family;
pub(crate) mod image_identity;
mod receipt;
mod source;

use crate::error::HostError;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) use archive_operation::{check_candidate, qualify_archives};

pub(super) async fn deadline<F: std::future::Future>(
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
    let result = Box::pin(velnor_runner_qualification::qualify(
        &docker,
        &source.qualification_identity(),
        runner_id,
        dind_id,
        [None, None],
        Instant::now() + Duration::from_secs(3600),
    ))
    .await?;
    source::revalidate(&source).await?;
    println!(
        "runner_listener_version={} runner_docker_version={} dind_docker_version={} dind_daemon_version={} runner_buildx_version={} runner_compose_version={} runner_node_version={}",
        result.versions.runner_listener,
        result.versions.runner_docker,
        result.versions.dind_docker,
        result.versions.dind_daemon,
        result.versions.runner_buildx,
        result.versions.runner_compose,
        result.versions.runner_node,
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
