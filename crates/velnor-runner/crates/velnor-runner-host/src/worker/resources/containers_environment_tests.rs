use std::path::Path;

use crate::error::HostError;
use crate::worker::{dind_create_for_identity, runner_create_for_identity};

use super::super::topology_matches;
use super::{identity, inspect_projection};

#[test]
fn runner_topology_matches_allowed_archive_environment() -> Result<(), HostError> {
    let spec = runner_create_for_identity(
        &identity()?,
        Some(Path::new("/var/lib/velnor/action-archives")),
    )?;
    let inspected = inspect_projection(&spec)?;

    assert!(topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_archive_environment_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(
        &identity()?,
        Some(Path::new("/var/lib/velnor/action-archives")),
    )?;
    let mut inspected = inspect_projection(&spec)?;
    let env = inspected
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .env
        .as_mut()
        .ok_or(HostError::Ownership)?;
    let cache = env
        .iter_mut()
        .find(|entry| entry.starts_with("ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE="))
        .ok_or(HostError::Ownership)?;
    *cache = "ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE=/tmp/wrong".to_owned();

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_loopback_endpoint_is_only_valid_for_its_private_dind_namespace() -> Result<(), HostError>
{
    let mut spec = runner_create_for_identity(&identity()?, None)?;
    spec.env.push("DOCKER_HOST=tcp://127.0.0.1:2375".to_owned());
    spec.loopback_docker_host = true;
    spec.network_mode = Some(format!("container:{}", "a".repeat(64)));
    let valid = inspect_projection(&spec)?;
    assert!(topology_matches(&spec, &valid)?);

    let mut wildcard = valid.clone();
    wildcard
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .env
        .as_mut()
        .ok_or(HostError::Ownership)?
        .iter_mut()
        .find(|entry| entry.starts_with("DOCKER_HOST="))
        .ok_or(HostError::Ownership)?
        .replace_range(.., "DOCKER_HOST=tcp://0.0.0.0:2375");
    assert!(!topology_matches(&spec, &wildcard)?);

    let mut host_network = spec.clone();
    host_network.network_mode = Some("host".to_owned());
    assert!(!topology_matches(&host_network, &valid)?);

    let mut non_loopback_runner = spec.clone();
    non_loopback_runner.loopback_docker_host = false;
    assert!(!topology_matches(&non_loopback_runner, &valid)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_unexpected_environment() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None)?;
    let valid = inspect_projection(&spec)?;

    let mut extra = valid.clone();
    extra
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .env
        .as_mut()
        .ok_or(HostError::Ownership)?
        .push("LD_PRELOAD=/tmp/override.so".to_owned());
    assert!(!topology_matches(&spec, &extra)?);

    let mut changed_path = valid.clone();
    changed_path
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .env
        .as_mut()
        .ok_or(HostError::Ownership)?[0] = "PATH=/tmp".to_owned();
    assert!(!topology_matches(&spec, &changed_path)?);

    let mut missing_path = valid;
    missing_path
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .env
        .as_mut()
        .ok_or(HostError::Ownership)?
        .clear();
    assert!(!topology_matches(&spec, &missing_path)?);
    Ok(())
}

#[test]
fn dind_topology_rejects_runner_docker_host_environment() -> Result<(), HostError> {
    let spec = dind_create_for_identity(&identity()?)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected
        .config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .env
        .as_mut()
        .ok_or(HostError::Ownership)?
        .push("DOCKER_HOST=tcp://127.0.0.1:2375".to_owned());

    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}
