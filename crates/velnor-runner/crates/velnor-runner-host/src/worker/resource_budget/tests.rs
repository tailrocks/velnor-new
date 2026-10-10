//! Resource budget unit checks. No Docker engine is required.

use super::{PairResourceBudget, ResourceBudgetConfig};
use crate::error::HostError;

fn config() -> ResourceBudgetConfig {
    ResourceBudgetConfig {
        runner_cpu_millicores: 1_000,
        runner_memory_bytes: 2_147_483_648,
        dind_cpu_millicores: 3_000,
        dind_memory_bytes: 6_442_450_944,
    }
}

#[test]
fn conversion_keeps_each_container_and_pair_budget_explicit() -> Result<(), HostError> {
    let budget = config().validate()?;
    assert_eq!(budget.runner().nano_cpus, 1_000_000_000);
    assert_eq!(budget.runner().memory_bytes, 2_147_483_648);
    assert_eq!(budget.dind().nano_cpus, 3_000_000_000);
    assert_eq!(budget.dind().memory_bytes, 6_442_450_944);
    assert_eq!(
        budget.pair(),
        PairResourceBudget {
            cpu_millicores: 4_000,
            memory_bytes: 8_589_934_592,
        }
    );
    Ok(())
}

#[test]
fn zero_or_overflowing_container_budgets_fail_closed() {
    let mut bad = config();
    bad.runner_cpu_millicores = 0;
    assert_eq!(bad.validate().err(), Some(HostError::Config));

    let mut bad = config();
    bad.dind_memory_bytes = 0;
    assert_eq!(bad.validate().err(), Some(HostError::Config));

    let mut bad = config();
    bad.runner_cpu_millicores = u64::MAX;
    assert_eq!(bad.validate().err(), Some(HostError::Config));

    let mut bad = config();
    bad.dind_memory_bytes = u64::MAX;
    assert_eq!(bad.validate().err(), Some(HostError::Config));
}

#[test]
fn aggregate_worker_budget_checks_multiplication_and_docker_ranges() -> Result<(), HostError> {
    let budget = config().validate()?;
    assert_eq!(budget.pair().docker_limits(0), Err(HostError::Config));
    assert_eq!(
        budget.pair().docker_limits(u32::MAX),
        Err(HostError::Config)
    );
    assert_eq!(
        PairResourceBudget {
            cpu_millicores: u64::MAX,
            memory_bytes: 1,
        }
        .docker_limits(2),
        Err(HostError::Config)
    );
    assert_eq!(
        PairResourceBudget {
            cpu_millicores: 1,
            memory_bytes: u64::MAX,
        }
        .docker_limits(2),
        Err(HostError::Config)
    );
    Ok(())
}

#[test]
fn docker_minimum_memory_and_guest_cpu_limits_are_checked() -> Result<(), HostError> {
    let mut bad = config();
    bad.runner_memory_bytes = 6_291_455;
    assert_eq!(bad.validate().err(), Some(HostError::Config));

    let budget = config().validate()?;
    assert_eq!(budget.validate_guest_cpu(Some(4)), Ok(()));
    assert_eq!(budget.validate_guest_cpu(Some(3)), Err(HostError::Config));
    assert_eq!(budget.validate_guest_cpu(Some(0)), Err(HostError::Config));
    assert_eq!(budget.validate_guest_cpu(None), Err(HostError::Config));
    assert_eq!(
        budget.validate_guest_cpu(Some(i64::MAX)),
        Err(HostError::Config)
    );
    Ok(())
}

#[test]
fn emitted_host_config_serializes_cpu_and_memory_limits() -> Result<(), HostError> {
    let budget = config().validate()?;
    let spec = crate::worker::runner_create(&crate::runner_plan("worker_a")?, budget)?;
    let create = crate::worker::bollard_create(&spec)?;
    let host = create.config.host_config.ok_or(HostError::Docker)?;
    let serialized = serde_json::to_value(host).map_err(|_| HostError::Docker)?;
    assert_eq!(serialized["NanoCpus"].as_i64(), Some(1_000_000_000));
    assert_eq!(serialized["Memory"].as_i64(), Some(2_147_483_648));
    assert_eq!(serialized["MemorySwap"].as_i64(), Some(2_147_483_648));
    assert_eq!(serialized["CgroupnsMode"].as_str(), Some("private"));
    assert_eq!(serialized["Privileged"].as_bool(), Some(false));

    let dind = crate::worker::dind_create("worker_a", budget)?;
    let created = crate::worker::bollard_create(&dind)?;
    let host = created.config.host_config.ok_or(HostError::Docker)?;
    let serialized = serde_json::to_value(host).map_err(|_| HostError::Docker)?;
    assert_eq!(serialized["NanoCpus"].as_i64(), Some(3_000_000_000));
    assert_eq!(serialized["Memory"].as_i64(), Some(6_442_450_944));
    assert_eq!(serialized["MemorySwap"].as_i64(), Some(6_442_450_944));
    assert_eq!(serialized["CgroupnsMode"].as_str(), Some("private"));
    assert_eq!(serialized["Privileged"].as_bool(), Some(true));
    let mut unbounded = spec;
    unbounded.resource_budget = None;
    assert_eq!(
        crate::worker::bollard_create(&unbounded),
        Err(HostError::Config)
    );
    Ok(())
}

#[test]
fn changed_config_reaches_both_actual_container_create_projections() -> Result<(), HostError> {
    let config = ResourceBudgetConfig {
        runner_cpu_millicores: 1_500,
        runner_memory_bytes: 3_221_225_472,
        dind_cpu_millicores: 2_500,
        dind_memory_bytes: 5_368_709_120,
    };
    let budget = config.validate()?;
    let runner = crate::worker::runner_create(&crate::runner_plan("worker_a")?, budget)?;
    let runner = crate::worker::bollard_create(&runner)?;
    let runner_host = runner.config.host_config.ok_or(HostError::Docker)?;
    assert_eq!(runner_host.nano_cpus, Some(1_500_000_000));
    assert_eq!(runner_host.memory, Some(3_221_225_472));
    assert_eq!(runner_host.memory_swap, Some(3_221_225_472));

    let dind = crate::worker::dind_create("worker_a", budget)?;
    let dind = crate::worker::bollard_create(&dind)?;
    let dind_host = dind.config.host_config.ok_or(HostError::Docker)?;
    assert_eq!(dind_host.nano_cpus, Some(2_500_000_000));
    assert_eq!(dind_host.memory, Some(5_368_709_120));
    assert_eq!(dind_host.memory_swap, Some(5_368_709_120));
    Ok(())
}

#[test]
fn resource_configuration_rejects_missing_malformed_and_overflowing_values() {
    assert!(toml::from_str::<ResourceBudgetConfig>(
        "runner_cpu_millicores = 1000\nrunner_memory_bytes = 2147483648\ndind_cpu_millicores = 3000"
    )
    .is_err());
    assert!(toml::from_str::<ResourceBudgetConfig>(
        "runner_cpu_millicores = 'many'\nrunner_memory_bytes = 2147483648\ndind_cpu_millicores = 3000\ndind_memory_bytes = 6442450944"
    )
    .is_err());
    let oversized = toml::from_str::<ResourceBudgetConfig>(
        "runner_cpu_millicores = 1000\nrunner_memory_bytes = 9223372036854775807\ndind_cpu_millicores = 3000\ndind_memory_bytes = 6442450944",
    );
    assert!(oversized.is_ok_and(|value| value.validate().is_err()));
}
