use std::collections::HashMap;

use serde_json::{Value, json};

use super::{CapturedRequest, DIND_ID, RUNNER_ID, route, support};

pub(in super::super) fn container_inspect(
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

pub(in super::super) fn mark_container_started(
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

pub(super) fn image_inspection(
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

pub(super) fn create_volume_response(body: &[u8]) -> Result<(u16, Vec<u8>), String> {
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

pub(super) fn create_container_response(
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
