// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::unix::net::UnixStream;

use serde_json::{Value, json};

use super::helper_seed::{self, INITIALIZER_ID, INITIALIZER_KEY, INITIALIZER_NAME, PUBLICATION_ID};
use super::{DIND_ID, PRIVATE_VOLUME, write_json};

pub(super) const ENGINE_DIGEST: &str =
    "b52bdf3b3388b9a5564e3c9873ed2a2c4ed4a5af1078ab1b1445e94c3dd0114f";
const NETWORK_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROLES: &[(&str, &str)] = &[
    ("", "shared-run"),
    ("-home", "runner-home"),
    ("-work", "runner-work"),
    ("-docker", "dind-data"),
    ("-metadata", "engine-metadata"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Publication,
    Initializer,
    Foreign(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Life {
    Created,
    Running,
    Gone,
}

#[derive(Clone, Copy)]
pub(crate) enum ListedId {
    Present,
    Missing,
}

#[derive(Clone)]
pub(crate) struct HelperSpec {
    pub(crate) kind: Kind,
    pub(crate) mounts: bool,
    pub(crate) listed: bool,
    pub(crate) listed_id: ListedId,
    pub(crate) life: Life,
    pub(crate) engine: Option<&'static str>,
    pub(crate) operation: Option<&'static str>,
    pub(crate) volumes_present: bool,
}

impl HelperSpec {
    pub(crate) const fn of(kind: Kind) -> Self {
        Self {
            kind,
            mounts: true,
            listed: true,
            listed_id: ListedId::Present,
            life: Life::Created,
            engine: None,
            operation: None,
            volumes_present: true,
        }
    }
}

struct Helper {
    id: String,
    name: String,
    listed_id: ListedId,
    body: Value,
    list_volumes: Vec<String>,
    mount_volumes: Vec<String>,
    running: bool,
    deleted: bool,
}

#[derive(Default)]
pub(super) struct VolumeState {
    pub(super) volumes: Vec<String>,
    pub(super) dind_removed: bool,
    pub(super) query: String,
    network_present: bool,
    helpers: Vec<Helper>,
    stops: Vec<String>,
    list_calls: usize,
}

fn volume(suffix: &str) -> String {
    format!("{PRIVATE_VOLUME}{suffix}")
}

fn build_helper(index: usize, spec: &HelperSpec, instance_id: &str) -> Helper {
    let metadata = helper_seed::metadata_volume();
    let (id, name, mut body, mounted) = match spec.kind {
        Kind::Publication => {
            let engine = spec
                .engine
                .map_or_else(helper_seed::publication_engine_label, str::to_owned);
            (
                PUBLICATION_ID.to_owned(),
                helper_seed::publication_name(),
                helper_seed::publication_inspect(instance_id, &engine),
                vec![metadata],
            )
        }
        Kind::Initializer => {
            let mounted = vec![volume("-home"), volume("-work")];
            let mut body = helper_seed::initializer_inspect(&mounted);
            body["Config"]["Labels"]["velnor.operation"] =
                json!(spec.operation.unwrap_or(INITIALIZER_KEY));
            (
                INITIALIZER_ID.to_owned(),
                INITIALIZER_NAME.to_owned(),
                body,
                mounted,
            )
        }
        Kind::Foreign(role) => {
            let id = format!("{index}{}", "3".repeat(63));
            let name = format!("velnor-foreign-{index}");
            let body = json!({"Id": id, "Name": format!("/{name}"), "Config": {"Labels": {"velnor.role": role, "velnor.engine": ENGINE_DIGEST}}, "State": {"Running": false, "Status": "created"}});
            (id, name, body, vec![metadata])
        }
    };
    let actual = if spec.mounts {
        mounted.clone()
    } else {
        vec!["unrelated-volume".to_owned()]
    };
    if matches!(spec.kind, Kind::Foreign(_)) || !spec.mounts {
        body["Mounts"] = Value::Array(
            actual
                .iter()
                .map(|name| json!({"Type": "volume", "Name": name, "Destination": "/opt/velnor", "RW": true}))
                .collect(),
        );
    }
    body["State"]["Running"] = json!(spec.life == Life::Running);
    Helper {
        id,
        name,
        listed_id: spec.listed_id,
        body,
        list_volumes: if spec.listed { mounted } else { Vec::new() },
        mount_volumes: actual,
        running: spec.life == Life::Running,
        deleted: spec.life == Life::Gone,
    }
}

impl VolumeState {
    pub(super) fn cancelled_before_start(specs: &[HelperSpec], instance_id: &str) -> Self {
        Self {
            volumes: if specs.iter().all(|spec| spec.volumes_present) {
                ROLES.iter().map(|(suffix, _)| volume(suffix)).collect()
            } else {
                Vec::new()
            },
            helpers: specs
                .iter()
                .enumerate()
                .map(|(index, spec)| build_helper(index, spec, instance_id))
                .collect(),
            network_present: true,
            ..Self::default()
        }
    }

    pub(super) const fn network_left(&self) -> usize {
        self.network_present as usize
    }

    fn present(&self) -> impl Iterator<Item = &Helper> {
        self.helpers.iter().filter(|helper| !helper.deleted)
    }

    fn find_mut(&mut self, key: &str) -> Option<&mut Helper> {
        self.helpers
            .iter_mut()
            .find(|helper| !helper.deleted && (helper.id == key || helper.name == key))
    }

    fn holds(&self, volume: &str) -> bool {
        self.present()
            .any(|helper| helper.mount_volumes.iter().any(|name| name == volume))
    }

    pub(super) fn listed(&self) -> usize {
        self.list_calls
    }

    pub(super) fn all_released(&self) -> bool {
        self.volumes.is_empty()
            && self.present().all(|helper| {
                helper
                    .mount_volumes
                    .iter()
                    .all(|name| name == "unrelated-volume")
            })
    }

    pub(super) fn present_count(&self) -> usize {
        self.present().count()
    }

    pub(super) fn counts(&self) -> (usize, usize, usize, usize) {
        let held = self
            .present()
            .filter(|helper| {
                helper
                    .mount_volumes
                    .iter()
                    .any(|name| name != "unrelated-volume")
            })
            .count();
        let other = self.present().count() - held;
        (held, other, self.stops.len(), self.volumes.len())
    }
}

fn no_content(stream: &mut UnixStream) -> std::io::Result<()> {
    stream
        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
    stream.shutdown(std::net::Shutdown::Both)
}

fn volume_body(name: &str, role: &str) -> Value {
    json!({"Name": name, "Driver": "local", "Mountpoint": format!("/var/lib/docker/volumes/{name}/_data"), "Labels": {"velnor.connection": "conn-test-1", "velnor.reservation": "2", "velnor.operation": "1", "velnor.role": role}, "Scope": "local", "Options": {}})
}

fn write_container_response(
    stream: &mut UnixStream,
    method: &str,
    path: &str,
    state: &mut VolumeState,
) -> std::io::Result<()> {
    let missing = json!({"message": "No such object"});
    match (method, path.split('/').collect::<Vec<_>>().as_slice()) {
        ("GET", ["containers", "json"]) => {
            state.list_calls += 1;
            let query = state.query.clone();
            let listed: Vec<Value> = state
                .present()
                .filter(|helper| {
                    helper
                        .list_volumes
                        .iter()
                        .any(|name| query.contains(name.as_str()))
                })
                .map(|helper| {
                    if matches!(helper.listed_id, ListedId::Missing) {
                        json!({})
                    } else {
                        json!({"Id": helper.id})
                    }
                })
                .collect();
            write_json(stream, 200, "OK", &Value::Array(listed))
        }
        ("GET", ["containers", key, "json"])
            if state.present().any(|h| h.id == *key || h.name == *key) =>
        {
            let body = state
                .present()
                .find(|helper| helper.id == *key || helper.name == *key)
                .map(|helper| helper.body.clone());
            write_json(stream, 200, "OK", &body.unwrap_or(missing))
        }
        ("POST", ["containers", key, "stop"])
            if state.present().any(|h| h.id == *key || h.name == *key) =>
        {
            state.stops.push((*key).to_owned());
            if let Some(helper) = state.find_mut(key) {
                helper.running = false;
                helper.body["State"]["Running"] = json!(false);
            }
            no_content(stream)
        }
        ("GET", ["containers", _, "json"]) => write_json(stream, 404, "Not Found", &missing),
        ("DELETE", ["containers", id]) if *id == DIND_ID => {
            state.dind_removed = true;
            no_content(stream)
        }
        ("DELETE", ["containers", key])
            if state.present().any(|h| h.id == *key || h.name == *key) =>
        {
            match state.find_mut(key) {
                Some(helper) if helper.running => write_json(
                    stream,
                    409,
                    "Conflict",
                    &json!({"message": "cannot remove a running container"}),
                ),
                Some(helper) => {
                    helper.deleted = true;
                    no_content(stream)
                }
                None => write_json(stream, 404, "Not Found", &missing),
            }
        }
        _ => {
            eprintln!("UNROUTED {method} {path}");
            write_json(stream, 404, "Not Found", &missing)
        }
    }
}

pub(super) fn write_volume_response(
    stream: &mut UnixStream,
    method: &str,
    path: &str,
    state: &mut VolumeState,
) -> std::io::Result<()> {
    let missing = json!({"message": "No such object"});
    if path.starts_with("containers/") {
        return write_container_response(stream, method, path, state);
    }
    match (method, path.split('/').collect::<Vec<_>>().as_slice()) {
        ("GET", ["volumes", name]) => {
            let role = ROLES
                .iter()
                .find(|(suffix, _)| volume(suffix) == *name)
                .filter(|_| state.volumes.iter().any(|volume| volume == name))
                .map(|(_, role)| *role);
            match role {
                Some(role) => write_json(stream, 200, "OK", &volume_body(name, role)),
                None => write_json(stream, 404, "Not Found", &missing),
            }
        }
        ("GET", ["networks", key])
            if state.network_present
                && (*key == super::seed::network_name() || *key == NETWORK_ID) =>
        {
            let labels: Value = serde_json::from_str(super::seed::NETWORK_LABELS)?;
            write_json(
                stream,
                200,
                "OK",
                &json!({"Name": super::seed::network_name(), "Id": NETWORK_ID, "Driver": "bridge", "Scope": "local", "Internal": false, "Attachable": false, "Ingress": false, "EnableIPv4": true, "EnableIPv6": false, "Containers": {}, "Labels": labels}),
            )
        }
        ("DELETE", ["networks", key]) if state.network_present && *key == NETWORK_ID => {
            state.network_present = false;
            no_content(stream)
        }
        ("DELETE", ["volumes", name]) if state.holds(name) => write_json(
            stream,
            409,
            "Conflict",
            &json!({"message": format!("remove {name}: volume is in use")}),
        ),
        ("DELETE", ["volumes", name]) => {
            state.volumes.retain(|volume| volume != name);
            no_content(stream)
        }
        _ => {
            eprintln!("UNROUTED {method} {path}");
            write_json(stream, 404, "Not Found", &missing)
        }
    }
}
