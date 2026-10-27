// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::super::http::{Reply, Request};
use super::super::state::{ApiFault, FakeVolume, State};
use super::{json_body, missing};

pub(super) fn route(request: &Request, path: &str, state: &mut State) -> Option<Reply> {
    match (request.method.as_str(), path) {
        ("POST", "volumes/create") => Some(create_volume(request, state)),
        ("GET", _) if path.starts_with("volumes/") => Some(inspect_volume(path, state)),
        ("DELETE", _) if path.starts_with("volumes/") => Some(remove_volume(path, state)),
        _ => None,
    }
}

fn create_volume(request: &Request, state: &mut State) -> Reply {
    let value = json_body(request);
    let name = string_field(&value, "Name").unwrap_or_default().to_owned();
    let labels = value.get("Labels").cloned().unwrap_or_else(|| json!({}));
    let volume = FakeVolume {
        name: name.clone(),
        labels: labels.clone(),
    };
    state.volumes.insert(name.clone(), volume);
    state.volume_creations += 1;
    if state.take_fault(ApiFault::LoseVolumeCreateResponse) {
        return Reply::bytes(500, Vec::new()).disconnect();
    }
    Reply::json(201, &volume_response(&name, &labels))
}

fn inspect_volume(path: &str, state: &mut State) -> Reply {
    let name = path.strip_prefix("volumes/").unwrap_or_default();
    let Some(volume) = state.volumes.get(name) else {
        return missing("volume");
    };
    Reply::json(200, &volume_response(&volume.name, &volume.labels))
}

fn volume_response(name: &str, labels: &Value) -> Value {
    let labels = labels
        .as_object()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    let volume = bollard::models::Volume {
        name: name.to_owned(),
        driver: "local".to_owned(),
        mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
        labels,
        scope: Some(bollard::models::VolumeScopeEnum::LOCAL),
        ..Default::default()
    };
    serde_json::to_value(volume).unwrap_or_else(|_| json!({}))
}

fn remove_volume(path: &str, state: &mut State) -> Reply {
    let name = path.strip_prefix("volumes/").unwrap_or_default();
    if volume_is_attached(name, state) {
        return super::server_error();
    }
    if state.take_fault(ApiFault::VolumeCleanupNotFound) {
        state.volumes.remove(name);
        return missing("volume");
    }
    if state.take_fault(ApiFault::VolumeCleanupServerError) {
        return super::server_error();
    }
    if state.take_fault(ApiFault::VolumeCleanupDisconnect) {
        return Reply::bytes(500, Vec::new()).disconnect();
    }
    if state.take_fault(ApiFault::VolumeCleanupStall) {
        state.volumes.remove(name);
        return Reply::bytes(204, Vec::new()).with_delay(std::time::Duration::from_secs(6));
    }
    if state.take_fault(ApiFault::VolumeCleanupStillPresent) {
        return Reply::bytes(204, Vec::new());
    }
    if state.volumes.remove(name).is_some() {
        Reply::bytes(204, Vec::new())
    } else {
        missing("volume")
    }
}

fn volume_is_attached(name: &str, state: &State) -> bool {
    state.containers.values().any(|container| {
        container.host_config["Mounts"]
            .as_array()
            .is_some_and(|mounts| mounts.iter().any(|mount| mount["Source"] == name))
    })
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}
