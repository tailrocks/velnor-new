use std::collections::HashMap;

use serde_json::{Value, json};

use super::super::super::support::{self, DOCKER_ROOT};
use super::*;

pub(super) fn answer(
    images: &crate::launch::VerifiedWorkerImages,
    initializer_image_id: Option<&str>,
    request: &CapturedRequest,
    volumes: &HashMap<String, Value>,
    networks: &HashMap<String, Value>,
    containers: &HashMap<String, Value>,
) -> Result<(u16, Vec<u8>), String> {
    let route = route(request);
    match (request.method.as_str(), route) {
        ("GET", "/info") => Ok((
            200,
            support::engine_info(
                images.engine_id(),
                DOCKER_ROOT,
                images.daemon_architecture(),
            )
            .into_bytes(),
        )),
        ("GET", path) if path.starts_with("/images/") => {
            if initializer_image_id.is_some_and(|image_id| path.contains(image_id)) {
                Ok((200, image_inspection(images, path)?.into_bytes()))
            } else {
                Ok((200, image_inspection(images, path)?.into_bytes()))
            }
        }
        ("GET", path) if path.starts_with("/volumes/") => {
            let name = path.trim_start_matches("/volumes/");
            match volumes.get(name) {
                Some(volume) => Ok((200, volume.to_string().into_bytes())),
                None => Ok((404, br#"{"message":"missing"}"#.to_vec())),
            }
        }
        ("GET", path) if path.starts_with("/networks/") => {
            let name = path.trim_start_matches("/networks/");
            match networks.get(name) {
                Some(network) => Ok((200, network.to_string().into_bytes())),
                None => Ok((404, br#"{"message":"missing"}"#.to_vec())),
            }
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
        ("POST", "/networks/create") => create_network_response(&request.body),
        ("POST", "/volumes/create") => create_volume_response(&request.body),
        ("POST", "/containers/create") => create_container_response(images, &request.body),
        ("POST", path) if path.ends_with("/start") => Ok((204, Vec::new())),
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
    id: &str,
    name: &str,
) -> Result<Value, String> {
    let image_id = request["Image"]
        .as_str()
        .ok_or_else(|| "Docker container image is absent".to_owned())?;
    let image = serde_json::from_str::<Value>(&image_inspection(images, image_id)?)
        .map_err(|error| error.to_string())?;
    let mut config = image["Config"].clone();
    config["Image"] = request["Image"].clone();
    for key in [
        "Cmd",
        "Entrypoint",
        "Env",
        "Labels",
        "User",
        "WorkingDir",
        "OpenStdin",
        "StdinOnce",
    ] {
        if !request[key].is_null() {
            config[key] = request[key].clone();
        }
    }
    if let (Some(base), Some(overrides)) = (
        image["Config"]["Labels"].as_object(),
        request["Labels"].as_object(),
    ) {
        let mut labels = base.clone();
        labels.extend(overrides.clone());
        config["Labels"] = Value::Object(labels);
    }
    let mounts = inspected_mounts(request)?;
    Ok(json!({
        "Id": id,
        "Name": format!("/{name}"),
        "Image": image_id,
        "Config": config,
        "HostConfig": request["HostConfig"],
        "Mounts": mounts,
        "State": {"Status": "created", "Running": false}
    }))
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
            container["State"] = json!({"Status": "running", "Running": true});
            found = true;
        }
    }
    if found {
        Ok(())
    } else {
        Err("Docker started a container that was not created".to_owned())
    }
}

fn inspected_mounts(request: &Value) -> Result<Vec<Value>, String> {
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
                "volume" => inspected["Name"] = json!(source),
                "bind" => inspected["Source"] = json!(source),
                _ => return Err("Docker create mount type is unsupported".to_owned()),
            }
            Ok(inspected)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let tmpfs = request["HostConfig"]["Tmpfs"]
        .as_object()
        .ok_or_else(|| "Docker create tmpfs list is absent".to_owned())?;
    inspected.extend(
        tmpfs
            .keys()
            .map(|target| json!({"Type": "tmpfs", "Destination": target, "RW": true})),
    );
    Ok(inspected)
}

fn image_inspection(
    images: &crate::launch::VerifiedWorkerImages,
    path: &str,
) -> Result<String, String> {
    let normalized = path.replace("%3A", ":").replace("%3a", ":");
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
    } else {
        return Err("Docker create used an image outside the verified bundle".to_owned());
    };
    Ok((
        201,
        json!({"Id": id, "Warnings": []}).to_string().into_bytes(),
    ))
}
