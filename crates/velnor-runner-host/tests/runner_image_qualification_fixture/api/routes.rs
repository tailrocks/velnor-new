// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "routes/containers.rs"]
mod containers;
#[path = "routes/exec.rs"]
mod exec;
#[path = "routes/images.rs"]
mod images;
#[path = "routes/probes.rs"]
mod probes;
#[path = "routes/volumes.rs"]
mod volumes;

use std::sync::{Arc, Mutex};

use super::http::{Reply, Request};
use super::state::State;

pub(super) fn route(request: &Request, state: &Arc<Mutex<State>>) -> Reply {
    let Ok(mut state) = state.lock() else {
        return Reply::json(
            500,
            &serde_json::json!({"message": "fixture state poisoned"}),
        );
    };
    state.api_requests += 1;
    let path = api_path(&request.path);
    if let Some(reply) = images::route(request, path, &mut state) {
        return reply;
    }
    if let Some(reply) = volumes::route(request, path, &mut state) {
        return reply;
    }
    if let Some(reply) = containers::route(request, path, &mut state) {
        return reply;
    }
    if let Some(reply) = exec::route(request, path, &mut state) {
        return reply;
    }
    Reply::json(
        404,
        &serde_json::json!({"message": "unknown fixture route"}),
    )
}

fn api_path(value: &str) -> &str {
    let path = value.split('?').next().unwrap_or(value);
    let stripped = path.strip_prefix('/').unwrap_or(path);
    match stripped.split_once('/') {
        Some((version, remainder))
            if version.starts_with('v')
                && version[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.') =>
        {
            remainder
        }
        _ => stripped,
    }
}

pub(super) fn json_body(request: &Request) -> serde_json::Value {
    serde_json::from_slice(&request.body).unwrap_or(serde_json::Value::Null)
}

pub(super) fn path_id(path: &str, prefix: &str, suffix: &str) -> Option<String> {
    let middle = path.strip_prefix(prefix)?.strip_suffix(suffix)?;
    (!middle.is_empty()).then(|| middle.to_owned())
}

pub(super) fn missing(name: &str) -> Reply {
    Reply::json(
        404,
        &serde_json::json!({"message": format!("{name} not found")}),
    )
}

pub(super) fn server_error() -> Reply {
    Reply::json(500, &serde_json::json!({"message": "fixture server error"}))
}
