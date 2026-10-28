// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use serde_json::json;

use super::super::http::{Reply, Request};
use super::super::state::{ApiFault, FakeExec, State};
use super::{json_body, missing};

pub(super) fn route(request: &Request, path: &str, state: &mut State) -> Option<Reply> {
    match (request.method.as_str(), path) {
        ("POST", _) if path.starts_with("containers/") && path.ends_with("/exec") => {
            Some(create_exec(request, path, state))
        }
        ("POST", _) if path.starts_with("exec/") && path.ends_with("/start") => {
            Some(start_exec(path, state))
        }
        ("GET", _) if path.starts_with("exec/") && path.ends_with("/json") => {
            Some(inspect_exec(path, state))
        }
        _ => None,
    }
}

fn create_exec(request: &Request, path: &str, state: &mut State) -> Reply {
    let container_id = path
        .strip_prefix("containers/")
        .and_then(|path| path.strip_suffix("/exec"))
        .unwrap_or_default()
        .to_owned();
    let valid_command = json_body(request)["Cmd"]
        == json!([
            "/usr/local/bin/docker",
            "--host",
            "unix:///var/run/docker.sock",
            "info",
            "--format",
            "{{.ID}}"
        ])
        && dind_socket_is_owned(&container_id, state);
    state.fault_reached |= valid_command && state.plan.inner_id == state.plan.outer_id;
    state.readiness_attempts += 1;
    let attempt = state.readiness_attempts;
    let empty_first = valid_command && state.plan.readiness_empty_first && attempt == 1;
    if empty_first {
        state.empty_readiness_attempts += 1;
    }
    let output = if !valid_command || empty_first {
        Vec::new()
    } else {
        format!("{}\n", state.plan.inner_id).into_bytes()
    };
    let id = state.next_exec_id();
    let unfinished = valid_command && state.plan.fault == Some(ApiFault::UnfinishedExec);
    let stalled = valid_command && state.plan.fault == Some(ApiFault::StallExecCreate);
    state.fault_reached |= unfinished || stalled;
    state.execs.insert(
        id.clone(),
        FakeExec {
            id: id.clone(),
            container_id,
            output,
            running: unfinished,
            exit_code: (!unfinished).then_some(i64::from(!valid_command)),
        },
    );
    let reply = Reply::json(201, &json!({"Id": id}));
    if stalled {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn dind_socket_is_owned(container_id: &str, state: &State) -> bool {
    let Some(container) = state.containers.get(container_id) else {
        return false;
    };
    if container.role != "dind" {
        return false;
    }
    let Some(source) = container.host_config["Mounts"]
        .as_array()
        .and_then(|mounts| mounts.iter().find(|mount| mount["Target"] == "/run"))
        .and_then(|mount| mount["Source"].as_str())
    else {
        return false;
    };
    state.volumes.get(source).is_some_and(|volume| {
        volume.labels["io.velnor.runner-image-qualifier.role"] == "socket"
            && volume.labels["io.velnor.runner-image-qualifier.run"]
                == container.labels["io.velnor.runner-image-qualifier.run"]
    })
}

fn start_exec(path: &str, state: &mut State) -> Reply {
    let Some(id) = path
        .strip_prefix("exec/")
        .and_then(|path| path.strip_suffix("/start"))
    else {
        return missing("exec");
    };
    let Some((output, succeeded)) = state
        .execs
        .get(id)
        .map(|exec| (exec.output.clone(), exec.exit_code == Some(0)))
    else {
        return missing("exec");
    };
    let stalled = succeeded && state.plan.fault == Some(ApiFault::StallExecStream);
    if stalled {
        state.fault_reached = true;
    }
    let reply = Reply::bytes(101, super::containers::stream_frame(1, &output)).upgrade();
    if stalled {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn inspect_exec(path: &str, state: &mut State) -> Reply {
    let Some(id) = path
        .strip_prefix("exec/")
        .and_then(|path| path.strip_suffix("/json"))
    else {
        return missing("exec");
    };
    let Some(exec) = state.execs.get(id) else {
        return missing("exec");
    };
    Reply::json(
        200,
        &json!({
            "ID": exec.id,
            "ContainerID": exec.container_id,
            "Running": exec.running,
            "ExitCode": exec.exit_code
        }),
    )
}
