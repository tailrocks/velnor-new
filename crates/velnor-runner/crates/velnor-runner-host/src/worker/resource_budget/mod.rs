//! Validated CPU and memory budgets for one runner plus its private `DinD`.

use serde::Deserialize;

use crate::error::HostError;

const MILLICORES_PER_CPU: u64 = 1_000;
const NANO_CPUS_PER_CPU: u64 = 1_000_000_000;
const NANOS_PER_MILLICORE: u64 = NANO_CPUS_PER_CPU / MILLICORES_PER_CPU;
const DOCKER_MIN_MEMORY_BYTES: u64 = 6_291_456;
#[cfg(test)]
const DOCKER_MIN_MEMORY_BYTES_I64: i64 = 6_291_456;

/// TOML values for the runner and the private `DinD` container.
///
/// CPU values are millicores and memory values are bytes. Each side is
/// required; there are deliberately no unlimited or zero-value defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceBudgetConfig {
    /// Runner container CPU allocation in millicores.
    pub(crate) runner_cpu_millicores: u64,
    /// Runner container memory limit in bytes.
    pub(crate) runner_memory_bytes: u64,
    /// Private `DinD` container CPU allocation in millicores.
    pub(crate) dind_cpu_millicores: u64,
    /// Private `DinD` container memory limit in bytes.
    pub(crate) dind_memory_bytes: u64,
}

/// Docker CPU and memory limits after checked unit conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DockerResourceLimits {
    /// Docker `HostConfig.nano_cpus`.
    pub(crate) nano_cpus: i64,
    /// Docker `HostConfig.memory` in bytes.
    pub(crate) memory_bytes: i64,
}

/// Check that an inspected Docker `HostConfig` carries finite supported limits.
#[cfg(test)]
#[must_use]
pub(crate) fn bounded_host_limits(
    nano_cpus: Option<i64>,
    memory_bytes: Option<i64>,
    memory_swap_bytes: Option<i64>,
) -> bool {
    matches!(
        (nano_cpus, memory_bytes, memory_swap_bytes),
        (Some(cpu), Some(memory), Some(swap))
            if cpu > 0 && memory >= DOCKER_MIN_MEMORY_BYTES_I64 && swap == memory
    )
}

/// Limits for one complete job pair, after checked addition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PairResourceBudget {
    /// Runner and private `DinD` total, in millicores.
    pub(crate) cpu_millicores: u64,
    /// Runner and private `DinD` total, in bytes.
    pub(crate) memory_bytes: u64,
}

/// Validated per-container limits and their aggregate pair budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResourceBudget {
    runner: DockerResourceLimits,
    dind: DockerResourceLimits,
    pair: PairResourceBudget,
}

impl ResourceBudgetConfig {
    /// Convert the configured units and validate both container and pair limits.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] for zero, malformed, or overflowing limits.
    pub(crate) fn validate(self) -> Result<ResourceBudget, HostError> {
        let runner = docker_limits(self.runner_cpu_millicores, self.runner_memory_bytes)?;
        let dind = docker_limits(self.dind_cpu_millicores, self.dind_memory_bytes)?;
        let pair = PairResourceBudget {
            cpu_millicores: self
                .runner_cpu_millicores
                .checked_add(self.dind_cpu_millicores)
                .ok_or(HostError::Config)?,
            memory_bytes: u64::try_from(runner.memory_bytes)
                .map_err(|_| HostError::Config)?
                .checked_add(u64::try_from(dind.memory_bytes).map_err(|_| HostError::Config)?)
                .ok_or(HostError::Config)?,
        };
        let _ = pair.docker_limits(1)?;
        Ok(ResourceBudget { runner, dind, pair })
    }
}

impl ResourceBudget {
    /// Runner container limits.
    #[must_use]
    pub(crate) const fn runner(self) -> DockerResourceLimits {
        self.runner
    }

    /// Private `DinD` container limits.
    #[must_use]
    pub(crate) const fn dind(self) -> DockerResourceLimits {
        self.dind
    }

    /// Aggregate runner plus private `DinD` limits for one job.
    #[must_use]
    pub(crate) const fn pair(self) -> PairResourceBudget {
        self.pair
    }

    /// Ensure one complete pair fits within the selected Docker daemon's CPU count.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] when the Docker CPU observation is absent,
    /// invalid, or too small for both configured container limits together.
    #[cfg(test)]
    pub(crate) fn validate_guest_cpu(self, guest_ncpu: Option<i64>) -> Result<(), HostError> {
        let guest_ncpu = guest_ncpu
            .filter(|count| *count > 0)
            .and_then(|count| u64::try_from(count).ok())
            .ok_or(HostError::Config)?;
        let max_nano_cpus = guest_ncpu
            .checked_mul(NANO_CPUS_PER_CPU)
            .ok_or(HostError::Config)?;
        let pair_nano_cpus = self
            .pair
            .cpu_millicores
            .checked_mul(NANOS_PER_MILLICORE)
            .ok_or(HostError::Config)?;
        if pair_nano_cpus <= max_nano_cpus {
            Ok(())
        } else {
            Err(HostError::Config)
        }
    }
}

impl PairResourceBudget {
    /// Aggregate limits for `jobs` concurrent pairs with checked multiplication.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Config`] for a zero count or arithmetic overflow.
    pub(crate) fn docker_limits(self, jobs: u32) -> Result<DockerResourceLimits, HostError> {
        if jobs == 0 || self.cpu_millicores == 0 || self.memory_bytes == 0 {
            return Err(HostError::Config);
        }
        let cpu_millicores = self
            .cpu_millicores
            .checked_mul(u64::from(jobs))
            .ok_or(HostError::Config)?;
        let memory_bytes = self
            .memory_bytes
            .checked_mul(u64::from(jobs))
            .ok_or(HostError::Config)?;
        let nano_cpus = cpu_millicores
            .checked_mul(NANOS_PER_MILLICORE)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(HostError::Config)?;
        Ok(DockerResourceLimits {
            nano_cpus,
            memory_bytes: i64::try_from(memory_bytes).map_err(|_| HostError::Config)?,
        })
    }
}

fn docker_limits(
    cpu_millicores: u64,
    memory_bytes: u64,
) -> Result<DockerResourceLimits, HostError> {
    if cpu_millicores == 0 || memory_bytes < DOCKER_MIN_MEMORY_BYTES {
        return Err(HostError::Config);
    }
    let nano_cpus = cpu_millicores
        .checked_mul(NANOS_PER_MILLICORE)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(HostError::Config)?;
    let memory_bytes = i64::try_from(memory_bytes).map_err(|_| HostError::Config)?;
    Ok(DockerResourceLimits {
        nano_cpus,
        memory_bytes,
    })
}

#[cfg(test)]
pub(crate) fn test_resource_budget() -> Result<ResourceBudget, HostError> {
    ResourceBudgetConfig {
        runner_cpu_millicores: 1_000,
        runner_memory_bytes: 2_147_483_648,
        dind_cpu_millicores: 3_000,
        dind_memory_bytes: 6_442_450_944,
    }
    .validate()
}

#[cfg(test)]
mod tests;
