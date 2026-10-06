//! Budgeted resource-limit reconciliation tests.

use crate::error::HostError;
use crate::worker::{dind_create_for_identity, runner_create_for_identity, test_resource_budget};

use super::super::topology_matches;
use super::{identity, inspect_projection};

#[test]
fn budgeted_topology_accepts_exact_resource_limits() -> Result<(), HostError> {
    let budget = test_resource_budget()?;
    let mut dind = dind_create_for_identity(&identity()?)?;
    dind.resource_budget = Some(budget);
    let mut inspected = inspect_projection(&dind)?;
    let host = inspected.host_config.as_mut().ok_or(HostError::Ownership)?;
    let expected = budget.dind();
    host.nano_cpus = Some(expected.nano_cpus);
    host.memory = Some(expected.memory_bytes);
    host.memory_swap = Some(expected.memory_bytes);
    assert!(topology_matches(&dind, &inspected)?);

    let mut runner = runner_create_for_identity(&identity()?, None)?;
    runner.resource_budget = Some(budget);
    let mut inspected = inspect_projection(&runner)?;
    let host = inspected.host_config.as_mut().ok_or(HostError::Ownership)?;
    let expected = budget.runner();
    host.nano_cpus = Some(expected.nano_cpus);
    host.memory = Some(expected.memory_bytes);
    host.memory_swap = Some(expected.memory_bytes);
    assert!(topology_matches(&runner, &inspected)?);
    Ok(())
}

#[test]
fn dind_topology_rejects_resource_limit_drift() -> Result<(), HostError> {
    let mut spec = dind_create_for_identity(&identity()?)?;
    spec.resource_budget = Some(test_resource_budget()?);
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
    let mut spec = runner_create_for_identity(&identity()?, None)?;
    spec.resource_budget = Some(test_resource_budget()?);
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
