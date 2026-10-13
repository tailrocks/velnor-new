use crate::OrchestratorError;
use crate::cover_identity::generator::sha256_hex;
use crate::internal::internal;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use velnor_actions_contract_config::config::{
    HostContainerProfile, MAX_CHECK_CONTAINER_PATH_BYTES, MAX_CHECK_CONTAINER_RUNTIME_ENTRIES,
    MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES,
};
use velnor_actions_mise::checks::ContainerObservation;

pub(crate) use crate::check_runtime::preparation::container::runtime::{
    RuntimeObservation, RuntimeRootEvidence, SocketEvidence,
};

const RUNTIME_FIELDS: [&str; 10] = [
    "context",
    "context_hash",
    "context_metadata",
    "docker_config",
    "endpoint",
    "runtime_dir",
    "runtime_entries",
    "runtime_link",
    "runtime_root",
    "socket",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeEntry {
    path: String,
    kind: String,
    owner: u32,
}

pub(super) fn validate(
    profile: &HostContainerProfile,
    runtime: &Value,
    before: &ContainerObservation,
    after: &ContainerObservation,
    before_runtime: &RuntimeObservation,
    after_runtime: &RuntimeObservation,
) -> Result<(), OrchestratorError> {
    let object = runtime
        .as_object()
        .ok_or_else(|| internal("container_runtime_shape"))?;
    validate_fields(object)?;
    let initial = parse_observation(object)?;
    if &initial != before_runtime || &initial != after_runtime {
        return Err(internal("container_runtime_snapshot_changed"));
    }
    validate_socket(profile, before, after, &initial.socket)?;
    validate_root(profile, initial.runtime_root.as_ref())?;
    validate_projection(profile, object, before, after)
}

fn validate_fields(object: &Map<String, Value>) -> Result<(), OrchestratorError> {
    if object.len() != RUNTIME_FIELDS.len()
        || RUNTIME_FIELDS
            .iter()
            .any(|field| !object.contains_key(*field))
    {
        return Err(internal("container_runtime_field"));
    }
    Ok(())
}

fn parse_observation(object: &Map<String, Value>) -> Result<RuntimeObservation, OrchestratorError> {
    let socket = serde_json::from_value(
        object
            .get("socket")
            .cloned()
            .ok_or_else(|| internal("container_runtime_socket"))?,
    )
    .map_err(|_| internal("container_runtime_socket"))?;
    let runtime_root = serde_json::from_value(
        object
            .get("runtime_root")
            .cloned()
            .ok_or_else(|| internal("container_runtime_root"))?,
    )
    .map_err(|_| internal("container_runtime_root"))?;
    Ok(RuntimeObservation {
        socket,
        runtime_root,
    })
}

fn validate_socket(
    profile: &HostContainerProfile,
    before: &ContainerObservation,
    after: &ContainerObservation,
    socket: &SocketEvidence,
) -> Result<(), OrchestratorError> {
    let path = profile.socket_path();
    if socket.path != path
        || before.endpoint != format!("unix://{path}")
        || after.endpoint != format!("unix://{path}")
        || socket.owner != profile.socket_uid()
        || socket.device == 0
        || socket.inode == 0
        || (socket.mode & 0o170_000) != 0o140_000
    {
        return Err(internal("container_runtime_socket_identity"));
    }
    Ok(())
}

fn validate_root(
    profile: &HostContainerProfile,
    root: Option<&RuntimeRootEvidence>,
) -> Result<(), OrchestratorError> {
    match (profile, root) {
        (HostContainerProfile::Docker { .. }, None) => Ok(()),
        (HostContainerProfile::Docker { .. }, Some(_)) => {
            Err(internal("container_runtime_unexpected_root"))
        }
        (HostContainerProfile::OrbStack { sdk, .. }, Some(root)) => {
            let expected = Path::new(&sdk.runtime_dir);
            if root.path != expected.to_str().unwrap_or("")
                || root.owner != sdk.runtime_uid
                || root.device == 0
                || root.inode == 0
                || (root.mode & 0o170_000) != 0o040_000
                || (root.mode & 0o022) != 0
                || !expected.is_absolute()
            {
                return Err(internal("container_runtime_root_identity"));
            }
            Ok(())
        }
        (HostContainerProfile::OrbStack { .. }, None) => {
            Err(internal("container_runtime_root_missing"))
        }
    }
}

fn validate_projection(
    profile: &HostContainerProfile,
    object: &Map<String, Value>,
    before: &ContainerObservation,
    after: &ContainerObservation,
) -> Result<(), OrchestratorError> {
    let endpoint = string_field(object, "endpoint")?;
    let context = string_field(object, "context")?;
    let context_hash = string_field(object, "context_hash")?;
    let docker_config = string_field(object, "docker_config")?;
    let context_metadata = string_field(object, "context_metadata")?;
    if endpoint != before.endpoint
        || endpoint != after.endpoint
        || context != profile.context()
        || context_hash != sha256_hex(context.as_bytes())
    {
        return Err(internal("container_runtime_projection_identity"));
    }
    let home = owned_home(before)?;
    if Path::new(docker_config) != home.join("docker")
        || Path::new(context_metadata)
            != home
                .join("docker")
                .join("contexts")
                .join("meta")
                .join(context_hash)
                .join("meta.json")
    {
        return Err(internal("container_runtime_projection_path"));
    }
    let runtime_dir = optional_path(object, "runtime_dir")?;
    let runtime_link = optional_path(object, "runtime_link")?;
    let entries = serde_json::from_value::<Vec<RuntimeEntry>>(
        object
            .get("runtime_entries")
            .cloned()
            .ok_or_else(|| internal("container_runtime_entries"))?,
    )
    .map_err(|_| internal("container_runtime_entries"))?;
    validate_entries(profile, &entries, endpoint, runtime_dir, runtime_link, home)
}

fn string_field<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, OrchestratorError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| internal("container_runtime_field"))
}

fn optional_path<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a str>, OrchestratorError> {
    let value = object
        .get(name)
        .ok_or_else(|| internal("container_runtime_field"))?;
    match value {
        Value::Null => Ok(None),
        Value::String(path) => Ok(Some(path)),
        _ => Err(internal("container_runtime_path")),
    }
}

fn validate_entries(
    profile: &HostContainerProfile,
    entries: &[RuntimeEntry],
    endpoint: &str,
    runtime_dir: Option<&str>,
    runtime_link: Option<&str>,
    home: &Path,
) -> Result<(), OrchestratorError> {
    if entries.len() > MAX_CHECK_CONTAINER_RUNTIME_ENTRIES {
        return Err(internal("container_runtime_entry_limit"));
    }
    let expected_owner = profile.socket_uid();
    let mut paths = BTreeSet::new();
    for entry in entries {
        if entry.owner != expected_owner
            || entry.path.is_empty()
            || entry.path.len()
                > if matches!(profile, HostContainerProfile::Docker { .. }) {
                    MAX_CHECK_CONTAINER_PATH_BYTES
                } else {
                    MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES
                }
            || entry.path.chars().any(char::is_control)
            || !paths.insert(entry.path.clone())
            || !matches!(
                entry.kind.as_str(),
                "directory" | "regular_file" | "unix_socket"
            )
        {
            return Err(internal("container_runtime_entry"));
        }
    }
    match profile {
        HostContainerProfile::Docker { .. } => {
            if runtime_dir.is_some()
                || runtime_link.is_some()
                || entries.len() != 1
                || entries[0].path != endpoint.strip_prefix("unix://").unwrap_or("")
                || entries[0].kind != "unix_socket"
            {
                return Err(internal("container_runtime_entry"));
            }
        }
        HostContainerProfile::OrbStack { sdk, .. } => {
            if entries.iter().any(|entry| !is_relative_clean(&entry.path)) {
                return Err(internal("container_runtime_entry_path"));
            }
            let runtime_dir = runtime_dir.ok_or_else(|| internal("container_runtime_directory"))?;
            let runtime_link = runtime_link.ok_or_else(|| internal("container_runtime_link"))?;
            if runtime_dir != sdk.runtime_dir
                || runtime_link != home.join(".orbstack").join("run").to_str().unwrap_or("")
            {
                return Err(internal("container_runtime_projection_path"));
            }
            validate_orb_entries(entries, endpoint, runtime_dir)?;
        }
    }
    Ok(())
}

fn validate_orb_entries(
    entries: &[RuntimeEntry],
    endpoint: &str,
    runtime_dir: &str,
) -> Result<(), OrchestratorError> {
    let socket = endpoint
        .strip_prefix("unix://")
        .ok_or_else(|| internal("container_runtime_endpoint"))?;
    let relative_socket = Path::new(socket)
        .strip_prefix(runtime_dir)
        .map_err(|_| internal("container_runtime_socket_path"))?;
    let relative_socket = relative_socket
        .to_str()
        .ok_or_else(|| internal("container_runtime_socket_path"))?;
    if !is_relative_clean(relative_socket)
        || !entries
            .iter()
            .any(|entry| entry.path == relative_socket && entry.kind == "unix_socket")
    {
        return Err(internal("container_runtime_socket_entry"));
    }
    Ok(())
}

fn is_relative_clean(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES
        && !path.contains('\\')
        && !path.contains("//")
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(part) if !part.is_empty()))
}

fn owned_home(observation: &ContainerObservation) -> Result<&Path, OrchestratorError> {
    let docker = &observation.docker_program;
    docker
        .parent()
        .and_then(Path::parent)
        .filter(|home| home.is_absolute())
        .ok_or_else(|| internal("container_runtime_home"))
}
