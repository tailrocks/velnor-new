// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::{ContainerCreateBody, HostConfig, SystemInfo};
use bollard::query_parameters::StartContainerOptions;
use futures_util::StreamExt;

use crate::error::HostError;

use super::resources::OwnedResources;
use super::source::SourceSnapshot;
use super::{append, create_options, deadline, mount, read_logs, remaining, wait_success};

const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);
const RUNNER_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const READINESS_TIMEOUT: Duration = Duration::from_secs(15);
const READINESS_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DaemonIdentity {
    pub(super) id: String,
    pub(super) root: String,
    pub(super) version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VersionEvidence {
    pub(super) runner_listener: String,
    pub(super) runner_docker: String,
    pub(super) dind_docker: String,
    pub(super) dind_daemon: String,
}

struct CommandSpec<'a> {
    image_id: &'a str,
    role: &'a str,
    entrypoint: &'a str,
    command: &'a [&'a str],
    socket_volume: Option<&'a str>,
    timeout: Duration,
}

pub(super) async fn inspect_outer(docker: &Docker) -> Result<DaemonIdentity, HostError> {
    let version = deadline(docker.version(), DOCKER_TIMEOUT)
        .await?
        .map_err(|_| HostError::Docker)?;
    let info = deadline(docker.info(), DOCKER_TIMEOUT)
        .await?
        .map_err(|_| HostError::Docker)?;
    daemon_identity(version.version.as_deref(), &info)
}

pub(super) async fn verify_versions(
    docker: &Docker,
    source: &SourceSnapshot,
    runner_id: &str,
    dind_id: &str,
    resources: &mut OwnedResources,
) -> Result<VersionEvidence, HostError> {
    let listener = run_command(
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
    let listener = single_line(&listener)?;
    if listener != source.runner_version {
        return Err(HostError::Identity);
    }
    for version in [&runner_docker, &dind_docker, &dind_daemon] {
        require_docker_version(version, &source.docker_version)?;
    }
    Ok(VersionEvidence {
        runner_listener: listener,
        runner_docker: single_line(&runner_docker)?,
        dind_docker: single_line(&dind_docker)?,
        dind_daemon: single_line(&dind_daemon)?,
    })
}

async fn docker_version_probe(
    docker: &Docker,
    resources: &mut OwnedResources,
    image_id: &str,
    role: &str,
    entrypoint: &str,
) -> Result<Vec<u8>, HostError> {
    run_command(
        docker,
        resources,
        CommandSpec {
            image_id,
            role,
            entrypoint,
            command: &["--version"],
            socket_volume: None,
            timeout: DOCKER_TIMEOUT,
        },
    )
    .await
}

async fn run_command(
    docker: &Docker,
    resources: &mut OwnedResources,
    command: CommandSpec<'_>,
) -> Result<Vec<u8>, HostError> {
    let end = Instant::now() + command.timeout;
    let volumes = command.socket_volume.into_iter().collect::<Vec<_>>();
    let name = resources.register_container(command.role)?;
    let mounts = command
        .socket_volume
        .map(|volume| vec![mount(volume, "/run")])
        .unwrap_or_default();
    let config = ContainerCreateBody {
        image: Some(command.image_id.to_owned()),
        entrypoint: Some(vec![command.entrypoint.to_owned()]),
        cmd: Some(
            command
                .command
                .iter()
                .map(|item| (*item).to_owned())
                .collect(),
        ),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        tty: Some(false),
        labels: Some(resources.labels(command.role)),
        host_config: Some(HostConfig {
            privileged: Some(false),
            network_mode: Some("none".to_owned()),
            port_bindings: None,
            mounts: (!mounts.is_empty()).then_some(mounts),
            auto_remove: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    let create = deadline(
        docker.create_container(Some(create_options(&name)), config),
        remaining(end)?,
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    resources.bind_container_id(&name, &create.id)?;
    let inspect = deadline(docker.inspect_container(&create.id, None), remaining(end)?)
        .await?
        .map_err(|_| HostError::Docker)?;
    resources.validate_inspect(
        &inspect,
        &name,
        &create.id,
        command.role,
        command.image_id,
        &volumes,
    )?;
    deadline(
        docker.start_container(&create.id, None::<StartContainerOptions>),
        remaining(end)?,
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    wait_success(docker, &create.id, remaining(end)?).await?;
    let output = read_logs(docker, &create.id, remaining(end)?).await?;
    let inspect = deadline(docker.inspect_container(&create.id, None), remaining(end)?)
        .await?
        .map_err(|_| HostError::Docker)?;
    resources.validate_inspect(
        &inspect,
        &name,
        &create.id,
        command.role,
        command.image_id,
        &volumes,
    )?;
    if inspect.state.and_then(|state| state.exit_code) != Some(0) {
        return Err(HostError::Docker);
    }
    Ok(output)
}

pub(super) async fn wait_inner_daemon(
    docker: &Docker,
    dind_container_id: &str,
) -> Result<String, HostError> {
    let end = Instant::now() + READINESS_TIMEOUT;
    loop {
        let now = Instant::now();
        if now >= end {
            return Err(HostError::DindReadiness);
        }
        let attempt = READINESS_ATTEMPT_TIMEOUT.min(end.saturating_duration_since(now));
        if let Ok(Ok(id)) = deadline(exec_daemon_id(docker, dind_container_id), attempt).await {
            let id = single_line_optional(&id)?;
            if !id.is_empty() {
                return Ok(id);
            }
        }
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::DindReadiness);
        }
        tokio::time::sleep(Duration::from_millis(250).min(remaining)).await;
    }
}

async fn exec_daemon_id(docker: &Docker, container_id: &str) -> Result<Vec<u8>, HostError> {
    let options = CreateExecOptions::<String> {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        tty: Some(false),
        cmd: Some(vec![
            "/usr/local/bin/docker".to_owned(),
            "--host".to_owned(),
            "unix:///var/run/docker.sock".to_owned(),
            "info".to_owned(),
            "--format".to_owned(),
            "{{.ID}}".to_owned(),
        ]),
        ..Default::default()
    };
    let created = deadline(
        docker.create_exec(container_id, options),
        READINESS_ATTEMPT_TIMEOUT,
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    if created.id.is_empty() {
        return Err(HostError::Identity);
    }
    let started = deadline(
        docker.start_exec(&created.id, Some(StartExecOptions::default())),
        READINESS_ATTEMPT_TIMEOUT,
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    let output = match started {
        StartExecResults::Attached { output, input } => {
            drop(input);
            read_exec_output(output).await?
        }
        StartExecResults::Detached => return Err(HostError::Docker),
    };
    let inspect = deadline(docker.inspect_exec(&created.id), READINESS_ATTEMPT_TIMEOUT)
        .await?
        .map_err(|_| HostError::Docker)?;
    if inspect.running != Some(false) || inspect.exit_code != Some(0) {
        return Err(HostError::Docker);
    }
    Ok(output)
}

async fn read_exec_output<S, E>(mut stream: S) -> Result<Vec<u8>, HostError>
where
    S: futures_util::Stream<Item = Result<LogOutput, E>> + Unpin,
{
    let mut output = Vec::with_capacity(64);
    while let Some(item) = stream.next().await {
        match item.map_err(|_| HostError::Docker)? {
            LogOutput::StdOut { message } => append(&mut output, &message)?,
            LogOutput::StdErr { message } if message.is_empty() => {}
            _ => return Err(HostError::Docker),
        }
    }
    Ok(output)
}

pub(super) async fn query_runner_daemon(
    docker: &Docker,
    runner_id: &str,
    socket_volume: &str,
    resources: &mut OwnedResources,
) -> Result<String, HostError> {
    let output = run_command(
        docker,
        resources,
        CommandSpec {
            image_id: runner_id,
            role: "runner-socket-query",
            entrypoint: "/usr/local/bin/docker",
            command: &[
                "--host",
                "unix:///var/run/docker.sock",
                "info",
                "--format",
                "{{.ID}}",
            ],
            socket_volume: Some(socket_volume),
            timeout: RUNNER_QUERY_TIMEOUT,
        },
    )
    .await?;
    single_line(&output)
}

fn daemon_identity(version: Option<&str>, info: &SystemInfo) -> Result<DaemonIdentity, HostError> {
    let id = info
        .id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(HostError::Identity)?;
    let root = info
        .docker_root_dir
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or(HostError::Identity)?;
    let version = version
        .filter(|value| !value.is_empty())
        .ok_or(HostError::Identity)?;
    Ok(DaemonIdentity {
        id: id.to_owned(),
        root: root.to_owned(),
        version: version.to_owned(),
    })
}

fn require_docker_version(actual: &[u8], expected: &str) -> Result<(), HostError> {
    let actual = single_line(actual)?;
    if actual.starts_with(&format!("Docker version {expected},")) {
        Ok(())
    } else {
        Err(HostError::Identity)
    }
}

fn single_line(bytes: &[u8]) -> Result<String, HostError> {
    let line = single_line_optional(bytes)?;
    if line.is_empty() {
        return Err(HostError::Identity);
    }
    Ok(line)
}

fn single_line_optional(bytes: &[u8]) -> Result<String, HostError> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Identity)?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0')) {
        return Err(HostError::Identity);
    }
    Ok(text.trim().to_owned())
}
