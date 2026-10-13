//! Idempotent container create checks.

use std::collections::{HashMap, HashSet};

use bollard::models::HostConfigCgroupnsModeEnum;

use super::super::mounts::{label_map, mount_source};
use super::super::{CreateProjection, identity_labels_match, launch_identity_labels_match};
use crate::error::HostError;

mod environment;
use environment::environment_matches;
#[path = "containers_limits.rs"]
mod limits;
use limits::resource_limits_match;

fn same_launch(expected: &HashMap<String, String>, actual: &HashMap<String, String>) -> bool {
    launch_identity_labels_match(expected, actual)
}

fn validate_existing_row(
    expected: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> Result<&'static str, HostError> {
    if !same_launch(expected, actual) {
        return Err(HostError::Ownership);
    }
    match actual.get("velnor.role").map(String::as_str) {
        Some("dind") => Ok("dind"),
        Some("runner") => Ok("runner"),
        _ => Err(HostError::Ownership),
    }
}

fn topology_matches(
    spec: &CreateProjection,
    found: &bollard::models::ContainerInspectResponse,
) -> Result<bool, HostError> {
    let Some(config) = found.config.as_ref() else {
        return Ok(false);
    };
    let Some(host) = found.host_config.as_ref() else {
        return Ok(false);
    };
    let labels = label_map(&spec.labels)?.ok_or(HostError::Ownership)?;
    if config.image.as_deref() != Some(spec.image.as_str())
        || !inspect_labels_match(&labels, found)
        || !execution_matches(spec, config)
        || !environment_matches(spec, config)?
        || host.privileged != Some(spec.privileged)
        || host.cgroupns_mode != Some(HostConfigCgroupnsModeEnum::PRIVATE)
        || !resource_limits_match(spec, host)
        || !network_matches(spec.network_mode.as_deref(), host.network_mode.as_deref())
    {
        return Ok(false);
    }
    Ok(expected_mounts(spec)? == observed_mounts(found)?)
}

fn execution_matches(spec: &CreateProjection, config: &bollard::models::ContainerConfig) -> bool {
    config.cmd.as_deref().unwrap_or_default() == spec.cmd.as_slice()
        && config.entrypoint.as_deref() == Some(spec.entrypoint.as_slice())
        && image_value_matches(spec.user.as_deref(), config.user.as_deref())
        && image_value_matches(spec.working_dir.as_deref(), config.working_dir.as_deref())
        && config.attach_stdin == Some(spec.open_stdin)
        && config.open_stdin == Some(spec.open_stdin)
        && config.stdin_once == Some(spec.open_stdin)
        && !config.tty.unwrap_or(false)
}

fn image_value_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected == actual.filter(|value| !value.is_empty())
}

fn inspect_labels_match(
    expected: &HashMap<String, String>,
    found: &bollard::models::ContainerInspectResponse,
) -> bool {
    let Some(actual) = found
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
    else {
        return false;
    };
    identity_labels_match(expected, actual)
}

fn network_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    match expected {
        Some(mode) => actual == Some(mode),
        None => actual.is_none_or(|mode| mode.is_empty() || mode == "default"),
    }
}

fn expected_mounts(
    spec: &CreateProjection,
) -> Result<HashSet<(String, String, String, bool)>, HostError> {
    let mut mounts = HashSet::with_capacity(spec.mounts.len() + spec.bind_mounts.len());
    for mount in &spec.mounts {
        let (_, source) = mount_source(&mount.source)?;
        mounts.insert(("volume".to_owned(), source, mount.target.clone(), true));
    }
    for mount in &spec.bind_mounts {
        mounts.insert((
            "bind".to_owned(),
            mount.source.clone(),
            mount.target.clone(),
            !mount.read_only,
        ));
    }
    Ok(mounts)
}

fn observed_mounts(
    found: &bollard::models::ContainerInspectResponse,
) -> Result<HashSet<(String, String, String, bool)>, HostError> {
    let records = found.mounts.as_ref().ok_or(HostError::Ownership)?;
    let mut mounts = HashSet::with_capacity(records.len());
    for mount in records {
        let typ = mount.typ.as_ref().ok_or(HostError::Ownership)?;
        let source = match typ.as_str() {
            "volume" => mount.name.as_ref(),
            "bind" => mount.source.as_ref(),
            _ => return Err(HostError::Ownership),
        }
        .ok_or(HostError::Ownership)?;
        mounts.insert((
            typ.clone(),
            source.clone(),
            mount.destination.clone().ok_or(HostError::Ownership)?,
            mount.rw.ok_or(HostError::Ownership)?,
        ));
    }
    Ok(mounts)
}

#[cfg(test)]
#[path = "containers_tests.rs"]
mod tests;
