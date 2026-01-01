//! Guest budget for one host. The configured ceiling stays explicit.
//!
//! CPU, memory, and disk each produce a slot count. The live count is the
//! minimum of those counts and the ceiling. Sampled admission requires every
//! disk value to be known. Mac host totals are not inputs.

use bollard::Docker;

use crate::docker_client::{DOCKER_OPERATION_TIMEOUT, docker_deadline_after};
use crate::error::HostError;
#[cfg(test)]
use crate::worker::resources::guest::{
    GuestResourceSample,
    sampler::{GuestSampleSnapshot, GuestSampleStatus},
};

/// vCPU reserved for one job, after one core is left for the daemon.
const CPU_PER_JOB: u32 = 4;
/// Bytes of guest memory reserved for one job.
const MEM_PER_JOB: u64 = 8 * 1024 * 1024 * 1024;
/// Free guest-disk bytes reserved for one job.
const DISK_PER_JOB: u64 = 20 * 1024 * 1024 * 1024;

/// Static slot count. `disk_free == None` leaves disk unconstrained.
///
/// An exhausted or zero budget yields no slots.
#[must_use]
pub fn guest_slots(ncpu: u32, mem_bytes: u64, disk_free: Option<u64>, ceiling: u32) -> u32 {
    let cpu = cpu_slots(ncpu);
    let mem = byte_slots(mem_bytes, MEM_PER_JOB);
    let disk = disk_free.map_or(ceiling, |bytes| byte_slots(bytes, DISK_PER_JOB));
    cpu.min(mem).min(disk).min(ceiling)
}

/// Discover guest capacity from the selected Docker engine.
///
/// # Errors
///
/// Returns [`HostError::Docker`] when the bounded info request fails or lacks
/// valid CPU and memory totals. Callers must stop admission on this error.
pub(crate) async fn discover_guest_capacity(
    docker: &Docker,
    ceiling: u32,
) -> Result<u32, HostError> {
    discover_guest_capacity_with_timeout(docker, ceiling, DOCKER_OPERATION_TIMEOUT).await
}

pub(crate) async fn discover_guest_capacity_with_timeout(
    docker: &Docker,
    ceiling: u32,
    timeout: std::time::Duration,
) -> Result<u32, HostError> {
    let info = docker_deadline_after(docker.info(), timeout)
        .await
        .map_err(|_| HostError::Docker)?
        .map_err(|_| HostError::Docker)?;
    guest_capacity_from_info(&info, ceiling)
}

fn guest_capacity_from_info(
    info: &bollard::models::SystemInfo,
    ceiling: u32,
) -> Result<u32, HostError> {
    let ncpu = info
        .ncpu
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(HostError::Docker)?;
    let memory = info
        .mem_total
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(HostError::Docker)?;
    if ncpu == 0 || memory == 0 {
        return Err(HostError::Docker);
    }
    Ok(guest_slots(ncpu, memory, None, ceiling))
}

const fn cpu_slots(ncpu: u32) -> u32 {
    ncpu.saturating_sub(1) / CPU_PER_JOB
}

fn byte_slots(bytes: u64, per_job: u64) -> u32 {
    let count = bytes / per_job;
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Return safe slots only when every current guest measurement is known.
///
/// Any positive memory PSI means the guest observed memory stalls. Admission
/// stays closed until a fresh sample reports no memory pressure.
#[cfg(test)]
pub(crate) fn sampled_guest_slots(snapshot: GuestSampleSnapshot, ceiling: u32) -> u32 {
    if snapshot.status != GuestSampleStatus::Available {
        return 0;
    }
    let Some((cpu, memory, psi, disk)) = sampled_inputs(snapshot.sample) else {
        return 0;
    };
    if psi > 0 {
        return 0;
    }
    guest_slots(cpu / 1000, memory, Some(disk), ceiling)
}

#[cfg(test)]
fn sampled_inputs(sample: GuestResourceSample) -> Option<(u32, u64, u16, u64)> {
    Some((
        sample.cpu_millicores?,
        sample.memory_available_bytes?,
        sample.memory_psi_some_avg10_bps?,
        sample.docker_root_free_bytes?,
    ))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bollard::models::SystemInfo;

    use super::{guest_capacity_from_info, guest_slots, sampled_guest_slots};
    use crate::error::HostError;
    use crate::worker::resources::guest::{
        GuestResourceSample,
        sampler::{GuestSampleFailure, GuestSampleSnapshot, GuestSampleStatus},
    };

    #[test]
    fn eighteen_cpu_guest_yields_four_slots_under_a_high_ceiling() {
        let mem = 121 * 1024 * 1024 * 1024;
        assert_eq!(guest_slots(18, mem, None, 8), 4);
        assert_eq!(guest_slots(18, mem, None, 4), 4);
        assert_eq!(guest_slots(18, mem, None, 2), 2);
    }

    #[test]
    fn memory_disk_and_zero_cpu_can_exhaust_the_count() {
        let mem = 10 * 1024 * 1024 * 1024;
        assert_eq!(guest_slots(18, mem, None, 8), 1);
        let disk = 25 * 1024 * 1024 * 1024;
        let big = 64 * 1024 * 1024 * 1024;
        assert_eq!(guest_slots(18, big, Some(disk), 8), 1);
        assert_eq!(guest_slots(4, big, None, 8), 0);
        assert_eq!(guest_slots(18, 0, None, 8), 0);
        assert_eq!(guest_slots(18, big, Some(0), 8), 0);
        assert_eq!(guest_slots(18, big, None, 0), 0);
    }

    #[test]
    fn zero_docker_cpu_or_memory_is_rejected_before_capacity_is_published() {
        let zero_cpu = SystemInfo {
            ncpu: Some(0),
            mem_total: Some(64 * 1024 * 1024 * 1024),
            ..Default::default()
        };
        let zero_memory = SystemInfo {
            ncpu: Some(18),
            mem_total: Some(0),
            ..Default::default()
        };

        assert_eq!(
            guest_capacity_from_info(&zero_cpu, 8),
            Err(HostError::Docker)
        );
        assert_eq!(
            guest_capacity_from_info(&zero_memory, 8),
            Err(HostError::Docker)
        );
    }

    #[test]
    fn sampled_pressure_closes_admission_until_all_guest_metrics_recover() {
        let healthy_sample = GuestResourceSample {
            cpu_millicores: Some(8000),
            memory_available_bytes: Some(16 * 1024 * 1024 * 1024),
            memory_psi_some_avg10_bps: Some(0),
            docker_root_free_bytes: Some(40 * 1024 * 1024 * 1024),
        };
        let healthy = snapshot(GuestSampleStatus::Available, healthy_sample);
        assert_eq!(sampled_guest_slots(healthy, 8), 1);

        let pressure = snapshot(
            GuestSampleStatus::Available,
            GuestResourceSample {
                memory_psi_some_avg10_bps: Some(1),
                ..healthy_sample
            },
        );
        assert_eq!(sampled_guest_slots(pressure, 8), 0);

        let missing_disk = snapshot(
            GuestSampleStatus::Available,
            GuestResourceSample {
                docker_root_free_bytes: None,
                ..healthy_sample
            },
        );
        assert_eq!(sampled_guest_slots(missing_disk, 8), 0);
        assert_eq!(
            sampled_guest_slots(snapshot(GuestSampleStatus::Pending, healthy_sample), 8),
            0
        );
        assert_eq!(
            sampled_guest_slots(
                snapshot(
                    GuestSampleStatus::Unavailable(GuestSampleFailure::Docker),
                    healthy_sample
                ),
                8
            ),
            0
        );
        assert_eq!(
            sampled_guest_slots(snapshot(GuestSampleStatus::Stale, healthy_sample), 8),
            0
        );
    }

    fn snapshot(status: GuestSampleStatus, sample: GuestResourceSample) -> GuestSampleSnapshot {
        GuestSampleSnapshot {
            sample,
            status,
            sampled_at: Some(Instant::now()),
        }
    }
}
