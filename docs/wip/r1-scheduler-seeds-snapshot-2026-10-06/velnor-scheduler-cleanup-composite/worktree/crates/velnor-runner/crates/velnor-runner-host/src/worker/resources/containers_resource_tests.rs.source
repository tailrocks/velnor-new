//! Resource-limit reconciliation tests.

use crate::error::HostError;
use crate::worker::{dind_create, runner_create_for_identity};

use super::super::{budget, identity, inspect_projection, topology_matches};

#[test]
fn runner_topology_matches_inspected_projection() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None, budget()?)?;
    let inspected = inspect_projection(&spec)?;
    assert!(topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_command_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None, budget()?)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected.config.as_mut().ok_or(HostError::Ownership)?.cmd = Some(vec!["/bin/sh".to_owned()]);
    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn dind_topology_matches_its_image_command_and_closed_stdin() -> Result<(), HostError> {
    let spec = dind_create(&identity()?, budget()?)?;
    let inspected = inspect_projection(&spec)?;
    assert!(topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn dind_topology_rejects_resource_limit_drift() -> Result<(), HostError> {
    let spec = dind_create(&identity()?, budget()?)?;
    let mut inspected = inspect_projection(&spec)?;
    inspected
        .host_config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .memory = Some(3_221_225_472);
    assert!(!topology_matches(&spec, &inspected)?);
    Ok(())
}

#[test]
fn runner_topology_rejects_cpu_memory_and_swap_drift() -> Result<(), HostError> {
    let spec = runner_create_for_identity(&identity()?, None, budget()?)?;
    let valid = inspect_projection(&spec)?;

    let mut wrong_cpu = valid.clone();
    wrong_cpu
        .host_config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .nano_cpus = Some(999_000_000);
    assert!(!topology_matches(&spec, &wrong_cpu)?);

    let mut wrong_memory = valid.clone();
    wrong_memory
        .host_config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .memory = Some(1_073_741_824);
    assert!(!topology_matches(&spec, &wrong_memory)?);

    let mut unbounded_swap = valid;
    unbounded_swap
        .host_config
        .as_mut()
        .ok_or(HostError::Ownership)?
        .memory_swap = None;
    assert!(!topology_matches(&spec, &unbounded_swap)?);
    Ok(())
}
