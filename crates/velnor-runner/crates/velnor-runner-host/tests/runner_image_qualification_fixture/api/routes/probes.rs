// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::super::state::{ApiFault, FakeContainer, Probe, State};

pub(super) fn probe_output(container: &FakeContainer, state: &State) -> (Vec<u8>, bool) {
    let role = container.role.as_str();
    if !valid_probe_config(container, state, role) {
        return (b"unrecognized probe configuration\n".to_vec(), false);
    }
    let mismatch = state.plan.version_mismatch;
    let runner_version = if mismatch == Some(Probe::RunnerListener) {
        "2.337.0"
    } else {
        state.plan.runner_version.as_str()
    };
    let docker_version = if matches!(
        (role, mismatch),
        ("runner-docker-cli", Some(Probe::RunnerDocker))
            | ("dind-docker-cli", Some(Probe::DindDocker))
            | ("dind-dockerd", Some(Probe::DindDockerd))
    ) {
        "Docker version 28.0.0, build fixture".to_owned()
    } else {
        format!(
            "Docker version {}, build fixture",
            state.plan.docker_version
        )
    };
    let output = match role {
        "runner-listener" => format!("{runner_version}\n"),
        "runner-docker-cli" | "dind-docker-cli" | "dind-dockerd" => {
            format!("{docker_version}\n")
        }
        "runner-socket-query" => {
            if state.plan.fault == Some(ApiFault::WrongRunnerSocketDaemon) {
                format!("{}\n", state.plan.outer_id)
            } else {
                format!(
                    "{}\n",
                    state
                        .plan
                        .runner_socket_id
                        .as_deref()
                        .unwrap_or(state.plan.inner_id.as_str())
                )
            }
        }
        _ => String::new(),
    };
    let fault_reached = matches!(
        (role, mismatch),
        ("runner-listener", Some(Probe::RunnerListener))
            | ("runner-docker-cli", Some(Probe::RunnerDocker))
            | ("dind-docker-cli", Some(Probe::DindDocker))
            | ("dind-dockerd", Some(Probe::DindDockerd))
    ) || (role == "runner-socket-query"
        && state.plan.fault == Some(ApiFault::WrongRunnerSocketDaemon));
    (output.into_bytes(), fault_reached)
}

fn valid_probe_config(container: &FakeContainer, state: &State, role: &str) -> bool {
    let Some((image_id, entrypoint, command)) = expected_probe(role, state) else {
        return false;
    };
    container.image_id == image_id
        && container.entrypoint == json!([entrypoint])
        && container.command == command
        && container.host_config["NetworkMode"] == "none"
        && container.host_config["Privileged"] == (role == "dind")
        && container.host_config["PortBindings"].is_null()
        && (role != "runner-socket-query" || shared_socket_mount(container, state))
}

fn expected_probe(role: &str, state: &State) -> Option<(String, &'static str, Value)> {
    match role {
        "runner-listener" => Some((
            state.plan.runner_id.clone(),
            "/home/runner/bin/Runner.Listener",
            json!(["--version"]),
        )),
        "runner-docker-cli" => Some((
            state.plan.runner_id.clone(),
            "/usr/local/bin/docker",
            json!(["--version"]),
        )),
        "dind-docker-cli" => Some((
            state.plan.dind_id.clone(),
            "/usr/local/bin/docker",
            json!(["--version"]),
        )),
        "dind-dockerd" => Some((
            state.plan.dind_id.clone(),
            "/usr/local/bin/dockerd",
            json!(["--version"]),
        )),
        "runner-socket-query" => Some((
            state.plan.runner_id.clone(),
            "/usr/local/bin/docker",
            json!([
                "--host",
                "unix:///var/run/docker.sock",
                "info",
                "--format",
                "{{.ID}}"
            ]),
        )),
        _ => None,
    }
}

fn shared_socket_mount(container: &FakeContainer, state: &State) -> bool {
    let Some(runner_mount) = mount_source(&container.host_config, "/run") else {
        return false;
    };
    let Some(dind) = state
        .containers
        .values()
        .find(|candidate| candidate.role == "dind")
    else {
        return false;
    };
    let Some(dind_mount) = mount_source(&dind.host_config, "/run") else {
        return false;
    };
    let Some(volume) = state.volumes.get(runner_mount) else {
        return false;
    };
    runner_mount == dind_mount
        && volume.labels["io.velnor.runner-image-qualifier.role"] == "socket"
        && volume.labels["io.velnor.runner-image-qualifier.run"]
            == dind.labels["io.velnor.runner-image-qualifier.run"]
}

fn mount_source<'a>(host_config: &'a Value, target: &str) -> Option<&'a str> {
    host_config["Mounts"]
        .as_array()?
        .iter()
        .find(|mount| mount["Target"] == target)?["Source"]
        .as_str()
}
