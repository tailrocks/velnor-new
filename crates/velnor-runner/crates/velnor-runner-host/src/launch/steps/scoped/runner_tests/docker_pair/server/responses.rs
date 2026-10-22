use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use super::super::super::support::{self, DOCKER_ROOT};
use super::*;

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

pub(super) fn container_inspect(
    images: &crate::launch::VerifiedWorkerImages,
    request: &Value,
    volumes: &HashMap<String, Value>,
    id: &str,
    name: &str,
) -> Result<Value, String> {
    let image_id = request["Image"]
        .as_str()
        .ok_or_else(|| "Docker container image is absent".to_owned())?;
    let image = serde_json::from_str::<Value>(&image_inspection(images, image_id)?)
        .map_err(|error| error.to_string())?;
    let config = merged_config(&image["Config"], request);
    let mounts = inspected_mounts(request, volumes)?;
    Ok(json!({
        "Id": id,
        "Name": format!("/{name}"),
        "Image": image_id,
        "RestartCount": 0,
        "Path": "/velnor/resource-probe",
        "Args": config["Cmd"],
        "Config": config,
        "HostConfig": request["HostConfig"],
        "Mounts": mounts,
        "State": {"Status": "created", "Running": false}
    }))
}

fn merged_config(image: &Value, request: &Value) -> Value {
    let mut merged = image.as_object().cloned().unwrap_or_default();
    if let Some(request) = request.as_object() {
        for (key, value) in request {
            if value.is_null() || matches!(key.as_str(), "HostConfig" | "NetworkingConfig") {
                continue;
            }
            if key == "Labels" {
                let mut labels = merged
                    .get(key)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if let Some(overrides) = value.as_object() {
                    labels.extend(overrides.clone());
                }
                merged.insert(key.clone(), Value::Object(labels));
            } else if key == "Env" {
                let mut environment = merged
                    .get(key)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let Some(overrides) = value.as_array() {
                    for entry in overrides {
                        let Some(entry) = entry.as_str() else {
                            continue;
                        };
                        let Some((name, _)) = entry.split_once('=') else {
                            continue;
                        };
                        if let Some(existing) = environment.iter_mut().find(|existing| {
                            existing.as_str().is_some_and(|existing| {
                                existing
                                    .split_once('=')
                                    .is_some_and(|(existing_name, _)| existing_name == name)
                            })
                        }) {
                            *existing = Value::String(entry.to_owned());
                        } else {
                            environment.push(Value::String(entry.to_owned()));
                        }
                    }
                }
                merged.insert(key.clone(), Value::Array(environment));
            } else {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(merged)
}

pub(super) fn mark_container_started(
    containers: &mut HashMap<String, Value>,
    request: &CapturedRequest,
) -> Result<(), String> {
    let id = route(request)
        .strip_prefix("/containers/")
        .and_then(|path| path.strip_suffix("/start"))
        .ok_or_else(|| "Docker start path has no container identity".to_owned())?;
    let mut found = false;
    for container in containers.values_mut() {
        if container["Id"].as_str() == Some(id) {
            let pid = match id {
                RUNNER_ID => 1_201,
                DIND_ID => 1_202,
                _ => 1_203,
            };
            container["State"] = json!({"Status": "running", "Running": true, "Pid": pid});
            found = true;
        }
    }
    if found {
        Ok(())
    } else {
        Err("Docker started a container that was not created".to_owned())
    }
}

fn inspected_mounts(
    request: &Value,
    volumes: &HashMap<String, Value>,
) -> Result<Vec<Value>, String> {
    let mut inspected = request["HostConfig"]["Mounts"]
        .as_array()
        .ok_or_else(|| "Docker create mount list is absent".to_owned())?
        .iter()
        .map(|mount| {
            let typ = mount["Type"]
                .as_str()
                .ok_or_else(|| "Docker create mount type is absent".to_owned())?;
            let source = mount["Source"]
                .as_str()
                .ok_or_else(|| "Docker create mount source is absent".to_owned())?;
            let destination = mount["Target"]
                .as_str()
                .ok_or_else(|| "Docker create mount target is absent".to_owned())?;
            let read_only = mount["ReadOnly"]
                .as_bool()
                .ok_or_else(|| "Docker create mount mode is absent".to_owned())?;
            let mut inspected = json!({
                "Type": typ,
                "Destination": destination,
                "RW": !read_only
            });
            match typ {
                "volume" => {
                    let volume = volumes
                        .get(source)
                        .ok_or_else(|| "Inspected container volume is not owned".to_owned())?;
                    inspected["Name"] = json!(source);
                    inspected["Source"] = volume["Mountpoint"].clone();
                    inspected["Driver"] = volume["Driver"].clone();
                }
                "bind" => inspected["Source"] = json!(source),
                _ => return Err("Docker create mount type is unsupported".to_owned()),
            }
            Ok(inspected)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(tmpfs) = request["HostConfig"]["Tmpfs"].as_object() {
        inspected.extend(
            tmpfs
                .keys()
                .map(|target| json!({"Type": "tmpfs", "Destination": target, "RW": true})),
        );
    }
    Ok(inspected)
}

fn image_inspection(
    images: &crate::launch::VerifiedWorkerImages,
    path: &str,
) -> Result<String, String> {
    let normalized = path.replace("%3A", ":").replace("%3a", ":");
    let probe_id = format!("sha256:{}", "a".repeat(64));
    if normalized.contains(&probe_id) {
        return crate::launch::resource_probe::intake_composition_image_inspect(
            images.source_sha(),
        )
        .map(|value| value.to_string())
        .map_err(|error| error.to_string());
    }
    let (_identity, inspection) = if normalized.contains(images.runner().runtime_id()) {
        (
            images.runner(),
            support::image_inspect(images, "velnor-runner", images.runner()),
        )
    } else if normalized.contains(images.dind().runtime_id()) {
        (
            images.dind(),
            support::image_inspect(images, "velnor-dind", images.dind()),
        )
    } else {
        return Err("Docker inspected an image outside the verified bundle".to_owned());
    };
    let mut inspection: Value =
        serde_json::from_str(&inspection).map_err(|error| error.to_string())?;
    inspection["Config"]["Env"] =
        json!(["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"]);
    Ok(inspection.to_string())
}

fn create_volume_response(body: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let name = value["Name"]
        .as_str()
        .ok_or_else(|| "Docker volume name is absent".to_owned())?;
    let response = json!({
        "Name": name,
        "Driver": "local",
        "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"),
        "Labels": value["Labels"],
        "Options": value.get("DriverOpts").cloned().unwrap_or_else(|| json!({})),
        "Scope": "local"
    });
    Ok((201, response.to_string().into_bytes()))
}

fn create_container_response(
    images: &crate::launch::VerifiedWorkerImages,
    body: &[u8],
) -> Result<(u16, Vec<u8>), String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let image = value["Image"]
        .as_str()
        .ok_or_else(|| "Docker container image is absent".to_owned())?;
    let id = if image == images.dind().runtime_id() {
        DIND_ID
    } else if image == images.runner().runtime_id() {
        RUNNER_ID
    } else if image == format!("sha256:{}", "a".repeat(64)) {
        let operation = value["Labels"]["velnor.operation"]
            .as_str()
            .ok_or_else(|| "Probe create operation label is absent".to_owned())?;
        return Ok((
            201,
            json!({"Id": format!("{operation}{}", "b".repeat(32)), "Warnings": []})
                .to_string()
                .into_bytes(),
        ));
    } else {
        return Err("Docker create used an image outside the verified bundle".to_owned());
    };
    Ok((
        201,
        json!({"Id": id, "Warnings": []}).to_string().into_bytes(),
    ))
}
