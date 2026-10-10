//! Static CPU and memory capacity from Docker and durable worker rows.

use bollard::Docker;

use crate::docker_client::{DOCKER_OPERATION_TIMEOUT, docker_deadline_after};
use crate::journal::Journal;
use crate::scale_set::EnsureError;
use crate::worker::ResourceBudget;

mod inspect;

const NANO_CPUS_PER_CPU: u64 = 1_000_000_000;
const NANO_CPUS_PER_MILLICORE: u64 = NANO_CPUS_PER_CPU / 1_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct OccupiedResources {
    permits: u32,
    nano_cpus: u64,
    memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GuestTotals {
    cpus: u32,
    memory_bytes: u64,
}

/// Inspect the selected Docker guest and calculate its configured-pair ceiling.
///
/// # Errors
///
/// Returns a generic error when engine identity, totals, ownership, or limits
/// cannot be established without ambiguity.
pub(super) async fn discover(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
) -> Result<u32, EnsureError> {
    discover_after(docker, journal, budget, ceiling, DOCKER_OPERATION_TIMEOUT).await
}

pub(super) async fn discover_after(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
    timeout: std::time::Duration,
) -> Result<u32, EnsureError> {
    let inspect = async {
        let info = docker_deadline_after(docker.info(), timeout)
            .await
            .map_err(|_| capacity_error("docker capacity"))?
            .map_err(|_| capacity_error("docker capacity"))?;
        let engine_id = info
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| capacity_error("docker capacity"))?;
        let bound_engine = journal
            .engine_id()
            .await
            .map_err(|_| capacity_error("docker capacity"))?;
        if engine_id != bound_engine {
            return Err(capacity_error("docker capacity"));
        }
        let totals = guest_totals(info.ncpu, info.mem_total)
            .ok_or_else(|| capacity_error("docker capacity"))?;
        let occupied = inspect::occupied(docker, journal, budget).await?;
        calculate(totals, occupied, budget, ceiling)
    };
    tokio::time::timeout(timeout, inspect)
        .await
        .map_err(|_| capacity_error("docker capacity"))?
}

fn guest_totals(cpus: Option<i64>, memory: Option<i64>) -> Option<GuestTotals> {
    Some(GuestTotals {
        cpus: u32::try_from(cpus?).ok().filter(|value| *value > 0)?,
        memory_bytes: u64::try_from(memory?).ok().filter(|value| *value > 0)?,
    })
}

fn calculate(
    guest: GuestTotals,
    occupied: OccupiedResources,
    budget: ResourceBudget,
    ceiling: u32,
) -> Result<u32, EnsureError> {
    let pair = budget.pair();
    let pair_cpu = pair
        .cpu_millicores
        .checked_mul(NANO_CPUS_PER_MILLICORE)
        .filter(|value| *value > 0)
        .ok_or_else(|| capacity_error("docker capacity"))?;
    let pair_memory = (pair.memory_bytes > 0)
        .then_some(pair.memory_bytes)
        .ok_or_else(|| capacity_error("docker capacity"))?;
    let total_cpu = u64::from(guest.cpus)
        .checked_mul(NANO_CPUS_PER_CPU)
        .ok_or_else(|| capacity_error("docker capacity"))?;
    let usable_cpu = total_cpu.saturating_sub(NANO_CPUS_PER_CPU);
    let cpu_slots = remaining(usable_cpu, occupied.nano_cpus) / pair_cpu;
    let memory_slots = remaining(guest.memory_bytes, occupied.memory_bytes) / pair_memory;
    let free_permits = ceiling.saturating_sub(occupied.permits);
    let additional = cpu_slots.min(memory_slots).min(u64::from(free_permits));
    let additional = u32::try_from(additional).map_err(|_| capacity_error("docker capacity"))?;
    let possible = occupied
        .permits
        .checked_add(additional)
        .ok_or_else(|| capacity_error("docker capacity"))?;
    Ok(possible.min(ceiling))
}

const fn remaining(total: u64, used: u64) -> u64 {
    total.saturating_sub(used)
}

fn capacity_error(step: &'static str) -> EnsureError {
    EnsureError::Unexpected { status: 0, step }
}

#[cfg(test)]
#[path = "resource_capacity_tests.rs"]
mod tests;
