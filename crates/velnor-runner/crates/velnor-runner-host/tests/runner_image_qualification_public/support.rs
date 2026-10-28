// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;
use std::time::Duration;

use super::TestResult;
use super::fixture::{
    self, ApiPlan, ContainerEvidence, DockerApiFixture, GitFixture, Probe, Snapshot,
};

pub(super) fn qualify_with_timeout_timed(
    git: &GitFixture,
    source_sha: &str,
    plan: ApiPlan,
    timeout: Duration,
) -> TestResult<(Output, Snapshot, Duration)> {
    let (output, api, elapsed) = invoke_with_api(git, source_sha, plan, timeout)?;
    Ok((output, api.snapshot()?, elapsed))
}

pub(super) fn invoke_with_api(
    git: &GitFixture,
    source_sha: &str,
    plan: ApiPlan,
    timeout: Duration,
) -> TestResult<(Output, DockerApiFixture, Duration)> {
    let api = DockerApiFixture::new(plan)?;
    let (output, elapsed) = fixture::invoke_with_timeout_timed(
        git.root(),
        source_sha,
        &digest_id('a'),
        &digest_id('b'),
        api.socket(),
        timeout,
    )?;
    Ok((output, api, elapsed))
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
    let output = String::from_utf8_lossy(&output.stdout);
    assert!(output.contains("qualified_runner_image_id"));
    for evidence in [
        "runner_listener_version=2.338.0",
        "runner_docker_version=Docker version 29.9.1,",
        "dind_docker_version=Docker version 29.9.1,",
        "dind_daemon_version=Docker version 29.9.1,",
        "runner_buildx_version=0.38.0",
        "runner_compose_version=5.6.0",
        "runner_node_version=26.12.0",
    ] {
        assert!(output.contains(evidence), "missing {evidence} in {output}");
    }
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
    for role in [
        "runner-listener",
        "runner-docker-cli",
        "dind-docker-cli",
        "dind-dockerd",
        "runner-buildx",
        "runner-compose",
        "runner-node",
    ] {
        assert_eq!(
            state
                .created
                .iter()
                .filter(|container| container.role == role)
                .count(),
            1,
            "{role} probe count"
        );
    }
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
        "runner-buildx" => (
            'a',
            "/usr/local/bin/docker",
            serde_json::json!(["buildx", "version"]),
        ),
        "runner-compose" => (
            'a',
            "/usr/local/bin/docker",
            serde_json::json!(["compose", "version"]),
        ),
        "runner-node" => ('a', "/usr/local/bin/node", serde_json::json!(["--version"])),
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

pub(super) fn version_mismatch_outcome(probe: Probe) -> TestResult<(bool, String)> {
    let git = GitFixture::new()?;
    let mut plan = ApiPlan::new(git.source_sha());
    plan.version_mismatch = Some(probe);
    plan.foreign_sentinel = true;
    let (output, state) = qualify(&git, git.source_sha(), plan)?;
    let issues = version_mismatch_issues(probe, &output, &state);
    let created_roles = state
        .created
        .iter()
        .map(|container| container.role.as_str())
        .collect::<Vec<_>>();
    let remaining_roles = state
        .containers
        .iter()
        .map(|container| container.role.as_str())
        .collect::<Vec<_>>();
    let outcome = format!(
        "probe={probe:?} exit={:?} fault_reached={} created_roles={created_roles:?} remaining_roles={remaining_roles:?} volumes={:?} images={:?} sentinel=({}, {}) api_requests={} readiness_attempts={} volume_creations={} issues={issues:?}",
        output.status.code(),
        state.fault_reached,
        state.volumes,
        state.images,
        state.foreign_container_present,
        state.foreign_volume_present,
        state.api_requests,
        state.readiness_attempts,
        state.volume_creations,
    );
    Ok((issues.is_empty(), outcome))
}

fn version_mismatch_issues(probe: Probe, output: &Output, state: &Snapshot) -> Vec<String> {
    let mut issues = Vec::new();
    let role = match probe {
        Probe::RunnerListener => "runner-listener",
        Probe::RunnerDocker => "runner-docker-cli",
        Probe::DindDocker => "dind-docker-cli",
        Probe::DindDockerd => "dind-dockerd",
        Probe::RunnerBuildx => "runner-buildx",
        Probe::RunnerCompose => "runner-compose",
        Probe::RunnerNode => "runner-node",
    };
    if output.status.success()
        || String::from_utf8_lossy(&output.stdout).contains("qualified_runner_image_id")
    {
        issues.push("wrong version was accepted".to_owned());
    }
    if !state.fault_reached || !state.created.iter().any(|container| container.role == role) {
        issues.push("selected CLI probe was not reached".to_owned());
    }
    if state.containers.len() != 1
        || !state.foreign_container_present
        || state.containers[0].name != "foreign-sentinel"
        || state.containers[0].role != "foreign"
        || state.volumes != ["foreign-sentinel"]
        || !state.foreign_volume_present
    {
        issues.push("owned cleanup or foreign sentinel state was wrong".to_owned());
    }
    if state.images.len() != 2
        || !state.images.contains(&digest_id('a'))
        || !state.images.contains(&digest_id('b'))
    {
        issues.push("qualified image IDs were not preserved".to_owned());
    }
    if state.api_requests == 0
        || state.volume_creations != 0
        || state.readiness_attempts != 0
        || state
            .created
            .iter()
            .any(|container| matches!(container.role.as_str(), "dind" | "runner-socket-query"))
    {
        issues.push("qualification entered the Dind or readiness phase".to_owned());
    }
    issues
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
