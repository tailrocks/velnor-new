//! Resource budget unit checks. No Docker engine is required.

use super::{PairResourceBudget, ResourceBudgetConfig};
use crate::error::HostError;
use crate::worker::CreateProjection;

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
    let spec = CreateProjection {
        name: "test-runner".to_owned(),
        image: "test:image".to_owned(),
        platform: "linux/amd64".to_owned(),
        env: Vec::new(),
        cmd: Vec::new(),
        entrypoint: vec!["/entrypoint".to_owned()],
        user: None,
        working_dir: None,
        labels: Vec::new(),
        mounts: Vec::new(),
        bind_mounts: Vec::new(),
        privileged: false,
        open_stdin: true,
        network_mode: None,
        resource_budget: Some(config().validate()?),
    };
    let create = crate::worker::bollard_create(&spec)?;
    let host = create.config.host_config.ok_or(HostError::Docker)?;
    let serialized = serde_json::to_value(host).map_err(|_| HostError::Docker)?;
    assert_eq!(serialized["NanoCpus"].as_i64(), Some(1_000_000_000));
    assert_eq!(serialized["Memory"].as_i64(), Some(2_147_483_648));
    assert_eq!(serialized["MemorySwap"].as_i64(), Some(2_147_483_648));
    assert_eq!(serialized["CgroupnsMode"].as_str(), Some("private"));
    assert_eq!(serialized["Privileged"].as_bool(), Some(false));

    let mut dind = crate::worker::dind_create("worker_a")?;
    dind.resource_budget = Some(config().validate()?);
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
    let unbounded_create = crate::worker::bollard_create(&unbounded)?;
    let unbounded_host = unbounded_create
        .config
        .host_config
        .ok_or(HostError::Docker)?;
    let unbounded_value = serde_json::to_value(unbounded_host).map_err(|_| HostError::Docker)?;
    assert_eq!(unbounded_value["NanoCpus"].as_i64(), None);
    assert_eq!(unbounded_value["Memory"].as_i64(), None);
    assert_eq!(unbounded_value["MemorySwap"].as_i64(), None);
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
