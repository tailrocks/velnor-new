use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use super::super::super::support::{self, DOCKER_ROOT};
use super::*;

#[path = "responses/container.rs"]
mod container;

pub(super) use container::{container_inspect, mark_container_started};
use container::{create_container_response, create_volume_response, image_inspection};

static VOLUME_INSPECTION: OnceLock<Mutex<Option<(String, InjectedVolumeInspection)>>> =
    OnceLock::new();

enum InjectedVolumeInspection {
    Existing(Value),
    Status(u16),
}

impl CapturedRequest {
    pub(crate) fn inject_preexisting_owned_volume(name: &str, labels: &Value) {
        inject_volume_inspection(
            name,
            InjectedVolumeInspection::Existing(json!({
                "Name": name,
                "Driver": "local",
                "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
                "Labels": labels,
                "Options": {},
                "Scope": "local"
            })),
        );
    }

    pub(crate) fn inject_volume_inspection_status(name: &str, status: u16) {
        inject_volume_inspection(name, InjectedVolumeInspection::Status(status));
    }
}

fn inject_volume_inspection(name: &str, response: InjectedVolumeInspection) {
    if let Ok(mut pending) = VOLUME_INSPECTION.get_or_init(|| Mutex::new(None)).lock() {
        *pending = Some((name.to_owned(), response));
    }
}

fn take_volume_inspection(name: &str) -> Option<InjectedVolumeInspection> {
    let pending = VOLUME_INSPECTION.get()?;
    let mut pending = pending.lock().ok()?;
    if pending.as_ref().is_some_and(|(target, _)| target == name) {
        pending.take().map(|(_, response)| response)
    } else {
        None
    }
}

fn volume_response(
    request: &CapturedRequest,
    volumes: &HashMap<String, Value>,
) -> Option<(u16, Vec<u8>)> {
    let path = route(request);
    match (request.method.as_str(), path) {
        ("GET", path) if path.starts_with("/volumes/") => {
            let name = path.trim_start_matches("/volumes/");
            match take_volume_inspection(name) {
                Some(InjectedVolumeInspection::Existing(volume)) => {
                    Some((200, volume.to_string().into_bytes()))
                }
                Some(InjectedVolumeInspection::Status(status)) => Some((
                    status,
                    br#"{"message":"injected volume inspection status"}"#.to_vec(),
                )),
                None => match volumes.get(name) {
                    Some(volume) => Some((200, volume.to_string().into_bytes())),
                    None => Some((404, br#"{"message":"missing"}"#.to_vec())),
                },
            }
        }
        ("DELETE", path) if path.starts_with("/volumes/") => Some((204, Vec::new())),
        _ => None,
    }
}

fn network_response(
    request: &CapturedRequest,
    networks: &HashMap<String, Value>,
) -> Option<(u16, Vec<u8>)> {
    let path = route(request);
    match (request.method.as_str(), path) {
        ("GET", path) if path.starts_with("/networks/") => {
            let name = path.trim_start_matches("/networks/");
            match networks.get(name) {
                Some(network) => Some((200, network.to_string().into_bytes())),
                None => Some((404, br#"{"message":"missing"}"#.to_vec())),
            }
        }
        ("DELETE", path) if path.starts_with("/networks/") => Some((204, Vec::new())),
        _ => None,
    }
}

pub(super) fn answer(
    images: &crate::launch::VerifiedWorkerImages,
    request: &CapturedRequest,
    volumes: &HashMap<String, Value>,
    networks: &HashMap<String, Value>,
    containers: &HashMap<String, Value>,
    probe_available_bytes: u64,
) -> Result<(u16, Vec<u8>), String> {
    let route = route(request);
    if let Some(response) = volume_response(request, volumes) {
        return Ok(response);
    }
    if let Some(response) = network_response(request, networks) {
        return Ok(response);
    }
    match (request.method.as_str(), route) {
        ("GET", "/info") => {
            let mut info: Value = serde_json::from_str(&support::engine_info(
                images.engine_id(),
                DOCKER_ROOT,
                images.daemon_architecture(),
            ))
            .map_err(|error| error.to_string())?;
            info["NCPU"] = json!(8);
            info["MemTotal"] = json!(68_719_476_736_u64);
            Ok((200, info.to_string().into_bytes()))
        }
        ("GET", path) if path.starts_with("/images/") => {
            Ok((200, image_inspection(images, path)?.into_bytes()))
        }
        ("GET", path) if path.starts_with("/containers/") && path.ends_with("/json") => {
            let name = path
                .trim_start_matches("/containers/")
                .trim_end_matches("/json");
            match containers.get(name) {
                Some(container) => Ok((200, container.to_string().into_bytes())),
                None => Ok((404, br#"{"message":"missing"}"#.to_vec())),
            }
        }
        ("GET", path) if path.starts_with("/containers/") && path.ends_with("/logs") => {
            let id = path
                .strip_prefix("/containers/")
                .and_then(|path| path.strip_suffix("/logs"))
                .ok_or_else(|| "The probe log route has no container ID.".to_owned())?;
            let pid = containers
                .get(id)
                .and_then(|container| container["State"]["Pid"].as_u64())
                .and_then(|pid| u32::try_from(pid).ok())
                .ok_or_else(|| "The probe log target has no inspected process ID.".to_owned())?;
            crate::launch::resource_probe::canonical_logs_response(pid, probe_available_bytes)
                .map(|response| (200, response))
                .map_err(|error| {
                    format!("The canonical probe logs could not be encoded. Error: {error}.")
                })
        }
        ("POST", "/networks/create") => create_network_response(&request.body),
        ("POST", "/volumes/create") => create_volume_response(&request.body),
        ("POST", "/containers/create") => create_container_response(images, &request.body),
        ("POST", path) if path.ends_with("/start") => Ok((204, Vec::new())),
        ("POST", path) if path.ends_with("/stop") => Ok((204, Vec::new())),
        ("POST", path) if path.starts_with("/containers/") && path.ends_with("/wait") => {
            Ok((200, br#"{"StatusCode":0}"#.to_vec()))
        }
        ("DELETE", path) if path.starts_with("/containers/") => Ok((204, Vec::new())),
        _ => Err("Docker API call is outside the scoped pair test".to_owned()),
    }
}

fn create_network_response(body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let name = value["Name"]
        .as_str()
        .ok_or_else(|| "Docker network name is absent".to_owned())?;
    let id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let response = json!({ "Id": id, "Warning": "", "Name": name });
    Ok((201, response.to_string().into_bytes()))
}

pub(super) fn response_id(body: &[u8]) -> Result<String, String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    value["Id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Docker create response has no ID".to_owned())
}
