//! Resource checks for exact Docker container reconciliation.

use bollard::models::HostConfig;

use crate::worker::CreateProjection;

pub(super) fn resource_limits_match(spec: &CreateProjection, host: &HostConfig) -> bool {
    let expected = if spec.privileged {
        spec.resource_budget.dind()
    } else {
        spec.resource_budget.runner()
    };
    host.nano_cpus == Some(expected.nano_cpus)
        && host.memory == Some(expected.memory_bytes)
        && host.memory_swap == Some(expected.memory_bytes)
}
