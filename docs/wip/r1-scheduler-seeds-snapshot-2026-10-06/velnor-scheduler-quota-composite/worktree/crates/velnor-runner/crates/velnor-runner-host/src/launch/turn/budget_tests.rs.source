//! Turn launch routes preserve pair budgets and configured capacity.

use crate::error::HostError;
use crate::journal::LaunchIdentity;
use crate::worker::{
    BollardCreate, ResourceBudget, ResourceBudgetConfig, bollard_create, dind_create,
    runner_create_for_identity,
};

use super::super::capacity::{CapacityHysteresis, GuestResourceLimits};
use super::policy::TurnPolicy;

type ContainerLimits = (Option<i64>, Option<i64>);

fn budget(
    runner_cpu_millicores: u64,
    runner_memory_bytes: u64,
    dind_cpu_millicores: u64,
    dind_memory_bytes: u64,
) -> Result<ResourceBudget, HostError> {
    ResourceBudgetConfig {
        runner_cpu_millicores,
        runner_memory_bytes,
        dind_cpu_millicores,
        dind_memory_bytes,
    }
    .validate()
}

fn pair_limits(budget: ResourceBudget) -> Result<(ContainerLimits, ContainerLimits), HostError> {
    let identity = LaunchIdentity::new(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        7,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "engine-test",
    )?;
    let runner = runner_create_for_identity(&identity, None, budget)?;
    let dind = dind_create(&identity, budget)?;
    Ok((
        container_limits(&bollard_create(&runner)?)?,
        container_limits(&bollard_create(&dind)?)?,
    ))
}

fn container_limits(create: &BollardCreate) -> Result<ContainerLimits, HostError> {
    let host = create
        .config
        .host_config
        .as_ref()
        .ok_or(HostError::Docker)?;
    Ok((host.nano_cpus, host.memory))
}

#[tokio::test]
async fn launch_routes_forward_distinct_pair_budgets() -> Result<(), HostError> {
    let initial_policy = TurnPolicy::new(budget(750, 1_073_741_824, 2_000, 2_147_483_648)?, 4);
    let initial_limits = initial_policy
        .scale_session(|budget| async move { pair_limits(budget) })
        .await?;
    assert_eq!(
        initial_limits,
        (
            (Some(750_000_000), Some(1_073_741_824)),
            (Some(2_000_000_000), Some(2_147_483_648)),
        )
    );

    let assignment_policy = TurnPolicy::new(budget(1_500, 3_221_225_472, 500, 1_073_741_824)?, 4);
    let assignment_limits = assignment_policy
        .drive_ready(|budget| async move { pair_limits(budget) })
        .await?;
    assert_eq!(
        assignment_limits,
        (
            (Some(1_500_000_000), Some(3_221_225_472)),
            (Some(500_000_000), Some(1_073_741_824)),
        )
    );

    let mut unknown_policy = CapacityHysteresis::new();
    let unknown = initial_policy.capacity(&mut unknown_policy, GuestResourceLimits::default(), 0);
    assert_eq!(unknown.ceiling, 1);
    assert_eq!(unknown.occupied, 0);

    let measured = GuestResourceLimits {
        cpu_workers: Some(8),
        memory_workers: Some(8),
        storage_workers: Some(8),
    };
    let _candidate = initial_policy.capacity(&mut unknown_policy, measured, 0);
    let capped = initial_policy.capacity(&mut unknown_policy, measured, 0);
    assert_eq!(capped.ceiling, 4);
    Ok(())
}
