//! Resource checks for exact Docker container reconciliation.

use bollard::models::HostConfig;

use crate::worker::{CreateProjection, bounded_host_limits};

pub(super) fn resource_limits_match(spec: &CreateProjection, host: &HostConfig) -> bool {
    match spec.resource_budget {
        Some(budget) => {
            let expected = if spec.privileged {
                budget.dind()
            } else {
                budget.runner()
            };
            host.nano_cpus == Some(expected.nano_cpus)
                && host.memory == Some(expected.memory_bytes)
                && host.memory_swap == Some(expected.memory_bytes)
        }
        None => bounded_host_limits(host.nano_cpus, host.memory, host.memory_swap),
    }
}
