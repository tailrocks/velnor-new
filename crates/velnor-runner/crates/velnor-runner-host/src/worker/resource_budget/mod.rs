// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::HostError;

const MILLICORES_PER_CPU: u64 = 1_000;
const NANO_CPUS_PER_CPU: u64 = 1_000_000_000;
const NANOS_PER_MILLICORE: u64 = NANO_CPUS_PER_CPU / MILLICORES_PER_CPU;
const DOCKER_MIN_MEMORY_BYTES: u64 = 6_291_456;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceBudgetConfig {
    pub(crate) runner_cpu_millicores: u64,

    pub(crate) runner_memory_bytes: u64,

    pub(crate) dind_cpu_millicores: u64,

    pub(crate) dind_memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DockerResourceLimits {
    pub(crate) nano_cpus: i64,

    pub(crate) memory_bytes: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PairResourceBudget {
    pub(crate) cpu_millicores: u64,

    pub(crate) memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    runner: DockerResourceLimits,
    dind: DockerResourceLimits,
    pair: PairResourceBudget,
}

impl ResourceBudgetConfig {
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
    #[must_use]
    pub(crate) const fn runner(self) -> DockerResourceLimits {
        self.runner
    }

    #[must_use]
    pub(crate) const fn dind(self) -> DockerResourceLimits {
        self.dind
    }

    #[must_use]
    pub(crate) const fn pair(self) -> PairResourceBudget {
        self.pair
    }
}

impl PairResourceBudget {
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
