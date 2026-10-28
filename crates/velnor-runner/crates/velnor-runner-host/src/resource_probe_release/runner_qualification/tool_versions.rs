// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use bollard::Docker;

use crate::error::HostError;

use super::probes::{self, CommandSpec};
use super::resources::OwnedResources;
use super::source::SourceSnapshot;

const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VersionEvidence {
    pub(super) runner_listener: String,
    pub(super) runner_docker: String,
    pub(super) dind_docker: String,
    pub(super) dind_daemon: String,
    pub(super) runner_buildx: String,
    pub(super) runner_compose: String,
    pub(super) runner_node: String,
}

struct ExistingVersionOutputs {
    listener: Vec<u8>,
    runner_docker: Vec<u8>,
    dind_docker: Vec<u8>,
    dind_daemon: Vec<u8>,
}

struct RunnerToolOutputs {
    buildx: Vec<u8>,
    compose: Vec<u8>,
    node: Vec<u8>,
}

pub(super) async fn verify_versions(
    docker: &Docker,
    source: &SourceSnapshot,
    runner_id: &str,
    dind_id: &str,
    resources: &mut OwnedResources,
) -> Result<VersionEvidence, HostError> {
    let existing = probe_existing_versions(docker, runner_id, dind_id, resources).await?;
    let listener = probes::single_line(&existing.listener)?;
    if listener != source.runner_version {
        return Err(HostError::Identity);
    }
    for version in [
        &existing.runner_docker,
        &existing.dind_docker,
        &existing.dind_daemon,
    ] {
        require_docker_version(version, &source.docker_version)?;
    }
    let runner_tools = probe_runner_tools(docker, runner_id, resources).await?;
    let buildx = require_buildx_version(&runner_tools.buildx, &source.buildx_version)?;
    let compose = require_compose_version(&runner_tools.compose, &source.compose_version)?;
    let node = require_node_version(&runner_tools.node, &source.node_version)?;

    Ok(VersionEvidence {
        runner_listener: listener,
        runner_docker: probes::single_line(&existing.runner_docker)?,
        dind_docker: probes::single_line(&existing.dind_docker)?,
        dind_daemon: probes::single_line(&existing.dind_daemon)?,
        runner_buildx: buildx,
        runner_compose: compose,
        runner_node: node,
    })
}

async fn probe_existing_versions(
    docker: &Docker,
    runner_id: &str,
    dind_id: &str,
    resources: &mut OwnedResources,
) -> Result<ExistingVersionOutputs, HostError> {
    let listener = probes::run_command(
        docker,
        resources,
        CommandSpec {
            image_id: runner_id,
            role: "runner-listener",
            entrypoint: "/home/runner/bin/Runner.Listener",
            command: &["--version"],
            socket_volume: None,
            timeout: DOCKER_TIMEOUT,
        },
    )
    .await?;
    let runner_docker = docker_version_probe(
        docker,
        resources,
        runner_id,
        "runner-docker-cli",
        "/usr/local/bin/docker",
    )
    .await?;
    let dind_docker = docker_version_probe(
        docker,
        resources,
        dind_id,
        "dind-docker-cli",
        "/usr/local/bin/docker",
    )
    .await?;
    let dind_daemon = docker_version_probe(
        docker,
        resources,
        dind_id,
        "dind-dockerd",
        "/usr/local/bin/dockerd",
    )
    .await?;
    Ok(ExistingVersionOutputs {
        listener,
        runner_docker,
        dind_docker,
        dind_daemon,
    })
}

async fn probe_runner_tools(
    docker: &Docker,
    runner_id: &str,
    resources: &mut OwnedResources,
) -> Result<RunnerToolOutputs, HostError> {
    let buildx = runner_tool_probe(
        docker,
        resources,
        runner_id,
        "runner-buildx",
        "/usr/local/bin/docker",
        &["buildx", "version"],
    )
    .await?;
    let compose = runner_tool_probe(
        docker,
        resources,
        runner_id,
        "runner-compose",
        "/usr/local/bin/docker",
        &["compose", "version"],
    )
    .await?;
    let node = runner_tool_probe(
        docker,
        resources,
        runner_id,
        "runner-node",
        "/usr/local/bin/node",
        &["--version"],
    )
    .await?;
    Ok(RunnerToolOutputs {
        buildx,
        compose,
        node,
    })
}

async fn docker_version_probe(
    docker: &Docker,
    resources: &mut OwnedResources,
    image_id: &str,
    role: &str,
    entrypoint: &str,
) -> Result<Vec<u8>, HostError> {
    runner_tool_probe(
        docker,
        resources,
        image_id,
        role,
        entrypoint,
        &["--version"],
    )
    .await
}

async fn runner_tool_probe(
    docker: &Docker,
    resources: &mut OwnedResources,
    image_id: &str,
    role: &str,
    entrypoint: &str,
    command: &[&str],
) -> Result<Vec<u8>, HostError> {
    probes::run_command(
        docker,
        resources,
        CommandSpec {
            image_id,
            role,
            entrypoint,
            command,
            socket_volume: None,
            timeout: DOCKER_TIMEOUT,
        },
    )
    .await
}

fn require_docker_version(actual: &[u8], expected: &str) -> Result<(), HostError> {
    let actual = probes::single_line(actual)?;
    if actual.starts_with(&format!("Docker version {expected},")) {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn require_buildx_version(actual: &[u8], expected: &str) -> Result<String, HostError> {
    let line = probes::single_line(actual)?;
    let expected = format!("v{expected}");
    let mut parts = line.split_whitespace();
    if parts.next() != Some("github.com/docker/buildx") || parts.next() != Some(expected.as_str()) {
        return Err(HostError::Identity);
    }
    let revision = parts.next().ok_or(HostError::Identity)?;
    if revision.is_empty() || parts.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(expected[1..].to_owned())
}

fn require_compose_version(actual: &[u8], expected: &str) -> Result<String, HostError> {
    let line = probes::single_line(actual)?;
    let expected = format!("v{expected}");
    let mut parts = line.split_whitespace();
    if parts.next() != Some("Docker")
        || parts.next() != Some("Compose")
        || parts.next() != Some("version")
        || parts.next() != Some(expected.as_str())
    {
        return Err(HostError::Identity);
    }
    Ok(expected[1..].to_owned())
}

fn require_node_version(actual: &[u8], expected: &str) -> Result<String, HostError> {
    let line = probes::single_line(actual)?;
    if line == format!("v{expected}") {
        Ok(expected.to_owned())
    } else {
        Err(HostError::Identity)
    }
}
