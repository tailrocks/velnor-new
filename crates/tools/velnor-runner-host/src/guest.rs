//! Guest budget for one host. The configured ceiling stays explicit.
//!
//! CPU, memory, and disk each produce a slot count. The live count is the
//! minimum of those counts and the ceiling. A missing disk sample does not
//! shrink the count. Mac host totals are not inputs.

use bollard::Docker;

use crate::docker_client::{DOCKER_OPERATION_TIMEOUT, docker_deadline_after};
use crate::error::HostError;

/// vCPU reserved for one job, after one core is left for the daemon.
const CPU_PER_JOB: u32 = 4;
/// Bytes of guest memory reserved for one job.
const MEM_PER_JOB: u64 = 8 * 1024 * 1024 * 1024;
/// Free guest-disk bytes reserved for one job.
const DISK_PER_JOB: u64 = 20 * 1024 * 1024 * 1024;

/// Live slot count. `disk_free == None` means disk was not observed.
///
/// Zero CPU yields one slot. A zero ceiling yields one slot.
#[must_use]
pub fn guest_slots(ncpu: u32, mem_bytes: u64, disk_free: Option<u64>, ceiling: u32) -> u32 {
    let ceiling = if ceiling == 0 { 1 } else { ceiling };
    let cpu = cpu_slots(ncpu);
    let mem = byte_slots(mem_bytes, MEM_PER_JOB);
    let disk = disk_free.map_or(ceiling, |bytes| byte_slots(bytes, DISK_PER_JOB));
    cpu.min(mem).min(disk).min(ceiling).max(1)
}

/// Discover guest capacity from the selected Docker engine.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the bounded info request fails or lacks
/// valid CPU and memory totals. Callers must stop admission on this error.
pub async fn discover_guest_capacity(docker: &Docker, ceiling: u32) -> Result<u32, HostError> {
    discover_guest_capacity_with_timeout(docker, ceiling, DOCKER_OPERATION_TIMEOUT).await
}

/// Discover guest capacity with an explicit info-request timeout.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the bounded info request fails or lacks
/// valid CPU and memory totals. Callers must stop admission on this error.
pub async fn discover_guest_capacity_with_timeout(
    docker: &Docker,
    ceiling: u32,
    timeout: std::time::Duration,
) -> Result<u32, HostError> {
    let info = docker_deadline_after(docker.info(), timeout)
        .await
        .map_err(|_| HostError::Docker)?
        .map_err(|_| HostError::Docker)?;
    let ncpu = info
        .ncpu
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(HostError::Docker)?;
    let memory = info
        .mem_total
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(HostError::Docker)?;
    Ok(guest_slots(ncpu, memory, None, ceiling))
}

const fn cpu_slots(ncpu: u32) -> u32 {
    if ncpu <= 1 {
        1
    } else {
        let usable = ncpu - 1;
        let slots = usable / CPU_PER_JOB;
        if slots == 0 { 1 } else { slots }
    }
}

fn byte_slots(bytes: u64, per_job: u64) -> u32 {
    let count = bytes / per_job;
    u32::try_from(count).unwrap_or(u32::MAX).max(1)
}

#[cfg(test)]
mod tests;
