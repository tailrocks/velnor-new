// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;
use std::time::Duration;

use super::TestResult;
use super::fixture::{self, ApiPlan, ContainerEvidence, DockerApiFixture, GitFixture, Snapshot};

pub(super) fn qualify_with_timeout_timed(
    git: &GitFixture,
    source_sha: &str,
    plan: ApiPlan,
    timeout: Duration,
) -> TestResult<(Output, Snapshot, Duration)> {
    let api = DockerApiFixture::new(plan)?;
    let (output, elapsed) = fixture::invoke_with_timeout_timed(
        git.root(),
        source_sha,
        &digest_id('a'),
        &digest_id('b'),
        api.socket(),
        timeout,
    )?;
    Ok((output, api.snapshot()?, elapsed))
}

pub(super) fn qualify(
    git: &GitFixture,
    source_sha: &str,
    plan: ApiPlan,
) -> TestResult<(Output, Snapshot)> {
    qualify_with_timeout(git, source_sha, plan, Duration::from_secs(40))
}

pub(super) fn qualify_with_timeout(
    git: &GitFixture,
    source_sha: &str,
    plan: ApiPlan,
    timeout: Duration,
) -> TestResult<(Output, Snapshot)> {
    let api = DockerApiFixture::new(plan)?;
    let output = fixture::invoke_with_timeout(
        git.root(),
        source_sha,
        &digest_id('a'),
        &digest_id('b'),
        api.socket(),
        timeout,
    )?;
    Ok((output, api.snapshot()?))
}

pub(super) fn assert_success(output: &Output, state: &Snapshot) {
    assert!(
        output.status.success(),
        "{}; api_requests={} image_inspections={} created={} volume_creations={} readiness_attempts={}",
        String::from_utf8_lossy(&output.stderr),
        state.api_requests,
        state.image_inspections,
        state.created.len(),
        state.volume_creations,
        state.readiness_attempts
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id"));
    assert_eq!(state.images.len(), 2);
    assert!(state.images.contains(&digest_id('a')));
    assert!(state.images.contains(&digest_id('b')));
    assert_eq!(
        state.containers.len(),
        usize::from(state.foreign_container_present)
    );
    assert!(state.containers.iter().all(|container| {
        container.name == "foreign-sentinel"
            && container.role == "foreign"
            && container.labels["io.velnor.runner-image-qualifier.run"] == "foreign-run"
            && container.labels["io.velnor.runner-image-qualifier.role"] == "foreign"
    }));
    assert_eq!(
        state.volumes.len(),
        usize::from(state.foreign_volume_present)
    );
    assert!(
        state
            .volumes
            .iter()
            .all(|volume| volume == "foreign-sentinel")
    );
}

pub(super) fn assert_source_rejected_before_api(output: &Output, state: &Snapshot) {
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("worker identity"));
    assert!(state.created.is_empty());
    assert!(state.containers.is_empty());
    assert_eq!(state.volumes, Vec::<String>::new());
    assert_eq!(state.readiness_attempts, 0);
    assert_eq!(state.image_inspections, 0);
    assert_eq!(state.api_requests, 0);
    assert_eq!(state.outer_info_calls, 0);
}

pub(super) fn assert_rejected_before_api(output: &Output, state: &Snapshot) {
    assert!(!output.status.success());
    assert_eq!(state.api_requests, 0);
    assert!(state.created.is_empty());
    assert!(state.containers.is_empty());
    assert_eq!(state.volumes, Vec::<String>::new());
}

pub(super) fn assert_failed_and_cleaned(output: &Output, state: &Snapshot) {
    assert!(!output.status.success(), "unexpected success");
    assert!(
        state.api_requests > 0,
        "Docker API fault was not reached: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(state.fault_reached, "configured fault was not reached");
    assert!(state.containers.is_empty(), "owned containers remain");
    assert_eq!(state.volumes, Vec::<String>::new(), "owned volumes remain");
    assert_eq!(state.images.len(), 2);
    assert!(state.images.contains(&digest_id('a')));
    assert!(state.images.contains(&digest_id('b')));
}

pub(super) fn assert_private_configs(state: &Snapshot) -> TestResult<()> {
    assert!(state.created.len() >= 6);
    let dind_socket = socket_mount_source(state, "dind")?;
    let runner_socket = socket_mount_source(state, "runner-socket-query")?;
    assert_eq!(dind_socket, runner_socket);
    for container in &state.created {
        assert_host_config(container);
        assert_image_command(container)?;
        assert_mount_config(container);
    }
    Ok(())
}

fn socket_mount_source<'a>(state: &'a Snapshot, role: &str) -> TestResult<&'a str> {
    state
        .created
        .iter()
        .find(|container| container.role == role)
        .and_then(|container| container.host_config["Mounts"][0]["Source"].as_str())
        .ok_or_else(|| "socket volume source is missing".into())
}

fn assert_host_config(container: &ContainerEvidence) {
    assert_eq!(
        container.host_config["NetworkMode"], "none",
        "{}",
        container.role
    );
    assert_eq!(
        container.host_config["Privileged"],
        container.role == "dind",
        "{}",
        container.role
    );
    assert!(container.host_config["PortBindings"].is_null());
}

fn assert_image_command(container: &ContainerEvidence) -> TestResult<()> {
    let (image, entrypoint, command) = match container.role.as_str() {
        "runner-listener" => (
            'a',
            "/home/runner/bin/Runner.Listener",
            serde_json::json!(["--version"]),
        ),
        "runner-docker-cli" => (
            'a',
            "/usr/local/bin/docker",
            serde_json::json!(["--version"]),
        ),
        "dind-docker-cli" => (
            'b',
            "/usr/local/bin/docker",
            serde_json::json!(["--version"]),
        ),
        "dind-dockerd" => (
            'b',
            "/usr/local/bin/dockerd",
            serde_json::json!(["--version"]),
        ),
        "runner-socket-query" => (
            'a',
            "/usr/local/bin/docker",
            serde_json::json!([
                "--host",
                "unix:///var/run/docker.sock",
                "info",
                "--format",
                "{{.ID}}"
            ]),
        ),
        "dind" => {
            assert_eq!(container.image_id, digest_id('b'));
            assert!(container.entrypoint.is_null());
            assert!(container.command.is_null());
            return Ok(());
        }
        role => return Err(format!("unexpected qualifier role {role}").into()),
    };
    assert_eq!(container.image_id, digest_id(image));
    assert_eq!(container.entrypoint, serde_json::json!([entrypoint]));
    assert_eq!(container.command, command);
    Ok(())
}

fn assert_mount_config(container: &ContainerEvidence) {
    let mounts = container.host_config["Mounts"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    match container.role.as_str() {
        "dind" => {
            assert_eq!(mounts.len(), 2);
            assert_eq!(mounts[0]["Target"], "/run");
            assert_eq!(mounts[1]["Target"], "/var/lib/docker");
        }
        "runner-socket-query" => {
            assert_eq!(mounts.len(), 1);
            assert_eq!(mounts[0]["Target"], "/run");
        }
        _ => assert_eq!(mounts, Vec::<serde_json::Value>::new()),
    }
}

pub(super) fn digest_id(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[derive(Clone, Copy)]
pub(super) enum DaemonMutation {
    SharedId,
    RunnerSocket,
}
