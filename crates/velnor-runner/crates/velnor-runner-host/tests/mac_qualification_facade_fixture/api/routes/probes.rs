// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_resource_probe::{CpuAncestorLimit, InodePolicy, ProbeRecord};

use super::super::state::{ApiFault, FakeContainer, Probe, State};

#[expect(
    clippy::too_many_lines,
    reason = "The peer supplies the complete external probe record and tool output."
)]
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
    let buildx_version = if mismatch == Some(Probe::RunnerBuildx) {
        "0.37.0"
    } else {
        state.plan.buildx_version.as_str()
    };
    let compose_version = if mismatch == Some(Probe::RunnerCompose) {
        "5.5.0"
    } else {
        state.plan.compose_version.as_str()
    };
    let node_version = if mismatch == Some(Probe::RunnerNode) {
        "26.11.0"
    } else {
        state.plan.node_version.as_str()
    };
    let output = match role {
        "resource-probe" => {
            let mut bytes = serde_json::to_vec(&ProbeRecord {
                schema_version: 5,
                docker_root_mount_target: "/velnor/docker-root".to_owned(),
                docker_root_mount_source: "/dev/vda1".to_owned(),
                docker_root_mount_root: "/var/lib/docker".to_owned(),
                docker_root_mount_id: 42,
                docker_root_device_major: 254,
                docker_root_device_minor: 1,
                docker_root_statx_device_major: 254,
                docker_root_statx_device_minor: 1,
                docker_root_fs_type_magic: 61267,
                docker_root_fs_type_name: "ext4".to_owned(),
                docker_root_filesystem_id: 9001,
                guest_boot_id: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
                docker_root_free_bytes: 50_000_000_000,
                docker_root_total_bytes: 100_000_000_000,
                docker_root_available_bytes: 45_000_000_000,
                docker_root_inode_total: 1_000_000,
                docker_root_inode_free: 900_000,
                docker_root_inode_available: 900_000,
                docker_root_inode_policy: InodePolicy::Finite,
                memory_available_bytes: 8_589_934_592,
                load_milli: 250,
                memory_psi_some_avg10_bps: Some(0),
                probe_cgroup_path: "/docker/probe-1".to_owned(),
                probe_cgroup_memory_max_bytes: Some(67_108_864),
                probe_cgroup_memory_current_bytes: 4_194_304,
                probe_cgroup_swap_max_bytes: Some(0),
                probe_cgroup_swap_current_bytes: 0,
                probe_cgroup_cpu_quota_micros: Some(25_000),
                probe_cgroup_cpu_period_micros: 100_000,
                probe_cgroup_cpuset_cpus_count: 4,
                cpu_ancestor_limits: vec![
                    CpuAncestorLimit {
                        path: "/".to_owned(),
                        cpuset_cpus_count: 4,
                        cpu_quota_micros: None,
                        cpu_period_micros: 100_000,
                    },
                    CpuAncestorLimit {
                        path: "/docker".to_owned(),
                        cpuset_cpus_count: 4,
                        cpu_quota_micros: None,
                        cpu_period_micros: 100_000,
                    },
                ],
                probe_host_pid: 1234,
                probe_namespace_pid: 1234,
                probe_cgroup_pids_max: Some(64),
            })
            .unwrap_or_default();
            bytes.push(b'\n');
            return (bytes, false);
        }
        "runner-listener" => format!("{runner_version}\n"),
        "runner-docker-cli" | "dind-docker-cli" | "dind-dockerd" => {
            format!("{docker_version}\n")
        }
        "runner-buildx" => format!(
            "github.com/docker/buildx v{buildx_version} 0dbe87f36ed472fe36b6796c3f70b4210a731fc5\n"
        ),
        "runner-compose" => format!("Docker Compose version v{compose_version}\n"),
        "runner-node" => format!("v{node_version}\n"),
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
            | ("runner-buildx", Some(Probe::RunnerBuildx))
            | ("runner-compose", Some(Probe::RunnerCompose))
            | ("runner-node", Some(Probe::RunnerNode))
    ) || (role == "runner-socket-query"
        && state.plan.fault == Some(ApiFault::WrongRunnerSocketDaemon));
    (output.into_bytes(), fault_reached)
}

fn valid_probe_config(container: &FakeContainer, state: &State, role: &str) -> bool {
    if role == "resource-probe" {
        return container.image_id == state.plan.probe_id
            && container.entrypoint == json!(["/velnor/resource-probe"])
            && container.command.is_null()
            && container.host_config["NetworkMode"] == "none"
            && container.host_config["Privileged"] == false
            && container.host_config["ReadonlyRootfs"] == true
            && container.host_config["PidMode"] == "host"
            && container.host_config["Mounts"][0]["Target"] == "/velnor/docker-root";
    }
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
        "runner-buildx" => Some((
            state.plan.runner_id.clone(),
            "/usr/local/bin/docker",
            json!(["buildx", "version"]),
        )),
        "runner-compose" => Some((
            state.plan.runner_id.clone(),
            "/usr/local/bin/docker",
            json!(["compose", "version"]),
        )),
        "runner-node" => Some((
            state.plan.runner_id.clone(),
            "/usr/local/bin/node",
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
