// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use serde_json::{Value, json};

use super::super::http::{Reply, Request};
use super::super::state::{
    ApiFault, ContainerEvidence, ContainerRole, DeferredMutation, FakeContainer, State,
};
use super::probes::probe_output;
use super::{json_body, missing, path_id, server_error};

pub(super) fn route(request: &Request, path: &str, state: &mut State) -> Option<Reply> {
    match (request.method.as_str(), path) {
        ("POST", "containers/create") => Some(create_container(request, state)),
        ("GET", _) if path.starts_with("containers/") && path.ends_with("/json") => {
            Some(inspect_container(path, state))
        }
        ("POST", _) if path.starts_with("containers/") && path.ends_with("/start") => {
            Some(start_container(path, state))
        }
        ("POST", _) if path.starts_with("containers/") && path.ends_with("/wait") => {
            Some(wait_container(path, state))
        }
        ("GET", _) if path.starts_with("containers/") && path.ends_with("/logs") => {
            Some(container_logs(path, state))
        }
        ("DELETE", _) if path.starts_with("containers/") => Some(remove_container(path, state)),
        _ => None,
    }
}

fn create_container(request: &Request, state: &mut State) -> Reply {
    let value = json_body(request);
    let name = query_value(&request.path, "name").unwrap_or_default();
    let id = state.next_container_id();
    let labels = value.get("Labels").cloned().unwrap_or_else(|| json!({}));
    let host_config = value
        .get("HostConfig")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let container = FakeContainer {
        id: id.clone(),
        name: name.clone(),
        image_id: value
            .get("Image")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        role: labels
            .get("io.velnor.runner-image-qualifier.role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        labels: labels.clone(),
        host_config: host_config.clone(),
        entrypoint: value.get("Entrypoint").cloned().unwrap_or(Value::Null),
        command: value.get("Cmd").cloned().unwrap_or(Value::Null),
        running: false,
        exit_code: None,
    };
    if state.take_fault(ApiFault::DisconnectThenLateContainerCreate) {
        state.pending_mutations += 1;
        state.pending_identities.push(format!("container {name}"));
        return Reply::bytes(500, Vec::new())
            .disconnect()
            .with_deferred_mutation_after(
                DeferredMutation::Container(container),
                Duration::from_secs(2),
            );
    }
    let delayed = [
        ApiFault::LateContainerCreate,
        ApiFault::LateContainerCreateReplacement,
        ApiFault::NeverSettleContainerCreate,
    ]
    .into_iter()
    .find(|fault| state.take_fault(*fault));
    if let Some(fault) = delayed {
        state.pending_mutations += 1;
        state.pending_identities.push(format!("container {name}"));
        let delay = if fault == ApiFault::NeverSettleContainerCreate {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(7)
        };
        return Reply::json(201, &json!({"Id": id, "Warnings": []}))
            .with_delay(delay)
            .with_deferred_mutation(DeferredMutation::Container(container));
    }
    let is_dind = container.role == "dind";
    state.created.push(evidence(&container));
    state.containers.insert(id.clone(), container);
    if let Some(reply) = unsettled_dind_reply(state, is_dind, &id, &name) {
        return reply;
    }
    if state.take_fault(ApiFault::LoseContainerCreateResponse) {
        return Reply::bytes(500, Vec::new()).disconnect();
    }
    let reply = Reply::json(201, &json!({"Id": id, "Warnings": []}));
    if state.take_fault(ApiFault::StallContainerCreate) {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn unsettled_dind_reply(state: &mut State, is_dind: bool, id: &str, name: &str) -> Option<Reply> {
    if !is_dind {
        return None;
    }
    let fault = [
        ApiFault::OwnedUnsettledDindCreate,
        ApiFault::OwnedUnsettledDindRemovalFailure,
    ]
    .into_iter()
    .any(|fault| state.take_fault(fault));
    if !fault {
        return None;
    }
    state.pending_mutations += 1;
    state.pending_identities.push(format!("container {name}"));
    Some(Reply::json(201, &json!({"Id": id, "Warnings": []})).with_delay(Duration::from_secs(60)))
}

fn inspect_container(path: &str, state: &mut State) -> Reply {
    let Some(id) = path_id(path, "containers/", "/json") else {
        return missing("container");
    };
    let Some(mut key) = container_key(state, &id) else {
        if state.pending_mutations > 0 {
            state.provisional_missing_responses += 1;
        }
        return missing("container");
    };
    if state.plan.fault == Some(ApiFault::LateContainerCreateReplacement)
        && state
            .containers
            .get(&key)
            .is_some_and(|container| container.name == id)
        && let Some(mut container) = state.containers.remove(&key)
    {
        let replacement = "e".repeat(64);
        container.id.clone_from(&replacement);
        "foreign".clone_into(&mut container.role);
        container.labels = json!({
            "io.velnor.runner-image-qualifier.run": "foreign-run",
            "io.velnor.runner-image-qualifier.role": "foreign"
        });
        state.containers.insert(replacement.clone(), container);
        key = replacement;
    }
    if state.take_fault(ApiFault::ForeignOwnerAtTrackedName)
        && let Some(container) = state.containers.get_mut(&key)
    {
        container.labels = json!({
            "io.velnor.runner-image-qualifier.run": "foreign-run",
            "io.velnor.runner-image-qualifier.role": "foreign"
        });
    }
    if state.take_fault(ApiFault::ReplacedContainerId)
        && let Some(mut container) = state.containers.remove(&key)
    {
        let replacement = "e".repeat(64);
        container.id.clone_from(&replacement);
        state.containers.insert(replacement.clone(), container);
        key = replacement;
    }
    let delayed = state.take_fault(ApiFault::StallContainerInspect);
    let Some(container) = state.containers.get(&key) else {
        return missing("container");
    };
    let mut host_config = container.host_config.clone();
    match state.plan.fault {
        Some(ApiFault::WrongOwnedConfig(ContainerRole::Dind)) if container.role == "dind" => {
            host_config["Privileged"] = Value::Bool(false);
            state.fault_reached = true;
        }
        Some(ApiFault::WrongOwnedConfig(ContainerRole::RunnerListener))
            if container.role == "runner-listener" =>
        {
            host_config["NetworkMode"] = Value::String("bridge".to_owned());
            state.fault_reached = true;
        }
        _ => {}
    }
    let state_value = json!({"Running": container.running, "ExitCode": container.exit_code});
    let reply = Reply::json(
        200,
        &json!({
            "Id": container.id,
            "Name": format!("/{}", container.name),
            "Image": container.image_id,
            "Config": {"Labels": container.labels},
            "HostConfig": host_config,
            "State": state_value
        }),
    );
    if delayed {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn start_container(path: &str, state: &mut State) -> Reply {
    let Some(id) = path_id(path, "containers/", "/start") else {
        return missing("container");
    };
    let Some(container) = state.containers.get_mut(&id) else {
        return missing("container");
    };
    if container.role == "dind" {
        container.running = true;
    } else {
        container.running = false;
        container.exit_code = Some(0);
    }
    let reply = Reply::bytes(204, Vec::new());
    if state.take_fault(ApiFault::StallContainerStart) {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn wait_container(path: &str, state: &mut State) -> Reply {
    let Some(id) = path_id(path, "containers/", "/wait") else {
        return missing("container");
    };
    if !state.containers.contains_key(&id) {
        return missing("container");
    }
    let mut reply = Reply::json(
        200,
        &json!({
            "StatusCode": i32::from(state.take_fault(ApiFault::NonzeroWait)),
            "Error": null
        }),
    );
    if state.take_fault(ApiFault::StallWait) {
        reply = reply.with_delay(Duration::from_secs(6));
    }
    reply
}

fn container_logs(path: &str, state: &mut State) -> Reply {
    let Some(id) = path_id(path, "containers/", "/logs") else {
        return missing("container");
    };
    let Some(container) = state.containers.get(&id) else {
        return missing("container");
    };
    let role = container.role.clone();
    let (mut output, fault_reached) = probe_output(container, state);
    state.fault_reached |= fault_reached;
    if state.take_fault(ApiFault::MalformedOutput) {
        output = vec![0xff, b'\n'];
    }
    if state.take_fault(ApiFault::OversizedOutput) {
        output = vec![b'x'; 5000];
    }
    let socket_failure =
        role == "runner-socket-query" && state.take_fault(ApiFault::RunnerSocketFailure);
    let stderr = state.take_fault(ApiFault::ProbeStderr) || socket_failure;
    let frame = stream_frame(if stderr { 2 } else { 1 }, &output);
    let reply = Reply::bytes(200, frame);
    if state.take_fault(ApiFault::StallLogs) {
        reply.with_delay(Duration::from_secs(6))
    } else {
        reply
    }
}

fn remove_container(path: &str, state: &mut State) -> Reply {
    let Some(id) = path_id(path, "containers/", "") else {
        return missing("container");
    };
    if state.plan.fault == Some(ApiFault::OwnedUnsettledDindRemovalFailure)
        && state
            .containers
            .get(&id)
            .is_some_and(|container| container.role == "dind")
    {
        return server_error();
    }
    if state.take_fault(ApiFault::CleanupNotFound) {
        state.containers.remove(&id);
        return missing("container");
    }
    if state.take_fault(ApiFault::CleanupServerError) {
        return server_error();
    }
    if state.take_fault(ApiFault::CleanupDisconnect) {
        return Reply::bytes(500, Vec::new()).disconnect();
    }
    if state.take_fault(ApiFault::CleanupStall) {
        return Reply::bytes(204, Vec::new()).with_delay(Duration::from_secs(6));
    }
    if state.take_fault(ApiFault::CleanupStillPresent) {
        return Reply::bytes(204, Vec::new());
    }
    if state.containers.remove(&id).is_some() {
        Reply::bytes(204, Vec::new())
    } else {
        missing("container")
    }
}

pub(super) fn stream_frame(stream_type: u8, payload: &[u8]) -> Vec<u8> {
    let Ok(length) = u32::try_from(payload.len()) else {
        return Vec::new();
    };
    let mut frame = vec![stream_type, 0, 0, 0];
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn container_key(state: &State, id_or_name: &str) -> Option<String> {
    state
        .containers
        .get_key_value(id_or_name)
        .map(|(id, _)| id.clone())
        .or_else(|| {
            state
                .containers
                .iter()
                .find(|(_, container)| container.name == id_or_name)
                .map(|(id, _)| id.clone())
        })
}

fn evidence(container: &FakeContainer) -> ContainerEvidence {
    ContainerEvidence {
        id: container.id.clone(),
        name: container.name.clone(),
        role: container.role.clone(),
        labels: container.labels.clone(),
        image_id: container.image_id.clone(),
        host_config: container.host_config.clone(),
        entrypoint: container.entrypoint.clone(),
        command: container.command.clone(),
    }
}

fn query_value(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let mut output = String::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(hex_value).unwrap_or_default();
            let low = bytes.next().and_then(hex_value).unwrap_or_default();
            output.push(char::from((high << 4) | low));
        } else if byte == b'+' {
            output.push(' ');
        } else {
            output.push(char::from(byte));
        }
    }
    output
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
