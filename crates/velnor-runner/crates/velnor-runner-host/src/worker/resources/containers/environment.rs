//! Exact effective environment for the pinned Ubuntu worker images.

use std::collections::HashMap;

use bollard::models::ContainerConfig;

use crate::docker_spec::RUNNER_DOCKER_HOST_ENV;
use crate::error::HostError;
use crate::worker::CreateProjection;

// Exact PATH from the linux/amd64 `ubuntu:26.04` image configuration.
const UBUNTU_PATH: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const ARCHIVE_CACHE_ENV: &str = "ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/opt/velnor/action-archives";

pub(super) fn environment_matches(
    spec: &CreateProjection,
    config: &ContainerConfig,
) -> Result<bool, HostError> {
    let Some(actual) = config.env.as_deref() else {
        return Ok(false);
    };
    Ok(expected_environment(spec)? == parse_environment(actual)?)
}

fn expected_environment(spec: &CreateProjection) -> Result<HashMap<String, String>, HostError> {
    let mut entries = vec![UBUNTU_PATH.to_owned()];
    let mut archive_cache_seen = false;
    let mut docker_host_seen = false;
    if !spec.open_stdin && (spec.loopback_docker_host || !spec.env.is_empty()) {
        return Err(HostError::Ownership);
    }
    for entry in &spec.env {
        if entry == ARCHIVE_CACHE_ENV && !archive_cache_seen {
            archive_cache_seen = true;
            entries.push(entry.clone());
        } else if entry == RUNNER_DOCKER_HOST_ENV && spec.loopback_docker_host && !docker_host_seen
        {
            docker_host_seen = true;
            entries.push(entry.clone());
        } else {
            return Err(HostError::Ownership);
        }
    }
    if spec.loopback_docker_host && !docker_host_seen {
        return Err(HostError::Ownership);
    }
    if !spec.loopback_docker_host && docker_host_seen {
        return Err(HostError::Ownership);
    }
    parse_environment(&entries)
}

fn parse_environment(entries: &[String]) -> Result<HashMap<String, String>, HostError> {
    let mut values = HashMap::with_capacity(entries.len());
    for entry in entries {
        let (key, value) = entry.split_once('=').ok_or(HostError::Ownership)?;
        if key.is_empty() || values.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(HostError::Ownership);
        }
    }
    Ok(values)
}
