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
pub(super) struct OccupiedResources {
    pub(super) permits: u32,
    pub(super) nano_cpus: u64,
    pub(super) memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GuestTotals {
    pub(super) cpus: u32,
    pub(super) memory_bytes: u64,
}

/// Resource-derived total job capacity. Zero remains distinct from the
/// one-worker poll header required before a usable guest sample exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct JobCapacity {
    total: u32,
    pair_fits: bool,
}

impl JobCapacity {
    /// Keep the queue header nonzero without granting a worker-start permit.
    #[must_use]
    pub(super) const fn poll_header(self) -> u32 {
        if self.total == 0 { 1 } else { self.total }
    }

    /// Whether static CPU and memory totals can fit one configured pair.
    #[must_use]
    pub(super) const fn permits_start(self) -> bool {
        self.pair_fits
    }

    /// Deny new starts when the selected engine is trusted but capacity is unknown.
    #[must_use]
    pub(super) const fn denied() -> Self {
        Self {
            total: 0,
            pair_fits: false,
        }
    }

    #[cfg(test)]
    pub(super) const fn total(self) -> u32 {
        self.total
    }
}

/// Whether capacity was measured on a trusted engine or admission must stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Discovery {
    /// A matching journal-bound engine and valid capacity measurement.
    Available(JobCapacity),
    /// Engine identity is verified, but capacity cannot be established.
    Unavailable,
    /// The selected Docker engine could not be verified against the journal.
    Untrusted(EnsureError),
}

/// Inspect the selected Docker guest and calculate its configured-pair ceiling.
pub(super) async fn discover(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
) -> Discovery {
    discover_after(docker, journal, budget, ceiling, DOCKER_OPERATION_TIMEOUT).await
}

pub(super) async fn discover_after(
    docker: &Docker,
    journal: &Journal,
    budget: ResourceBudget,
    ceiling: u32,
    timeout: std::time::Duration,
) -> Discovery {
    let started = tokio::time::Instant::now();
    let Ok(Ok(info)) = docker_deadline_after(docker.info(), timeout).await else {
        return Discovery::Untrusted(capacity_error("docker capacity"));
    };
    let Some(engine_id) = info.id.as_deref().filter(|id| !id.is_empty()) else {
        return Discovery::Untrusted(capacity_error("docker capacity"));
    };
    let remaining = timeout.saturating_sub(started.elapsed());
    let Ok(Ok(bound_engine)) = tokio::time::timeout(remaining, journal.engine_id()).await else {
        return Discovery::Untrusted(capacity_error("docker capacity"));
    };
    if engine_id != bound_engine {
        return Discovery::Untrusted(capacity_error("docker capacity"));
    }

    let remaining = timeout.saturating_sub(started.elapsed());
    let inspect = async {
        let totals = guest_totals(info.ncpu, info.mem_total)
            .ok_or_else(|| capacity_error("docker capacity"))?;
        let occupied = inspect::occupied(docker, journal, budget, ceiling).await?;
        calculate(totals, occupied, budget, ceiling)
    };
    match tokio::time::timeout(remaining, inspect).await {
        Ok(Ok(capacity)) => Discovery::Available(capacity),
        _ => Discovery::Unavailable,
    }
}

fn guest_totals(cpus: Option<i64>, memory: Option<i64>) -> Option<GuestTotals> {
    Some(GuestTotals {
        cpus: u32::try_from(cpus?).ok().filter(|value| *value > 0)?,
        memory_bytes: u64::try_from(memory?).ok().filter(|value| *value > 0)?,
    })
}

pub(super) fn calculate(
    guest: GuestTotals,
    occupied: OccupiedResources,
    budget: ResourceBudget,
    ceiling: u32,
) -> Result<JobCapacity, EnsureError> {
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
    let pair_fits = usable_cpu >= pair_cpu && guest.memory_bytes >= pair_memory;
    let cpu_slots = remaining(usable_cpu, occupied.nano_cpus) / pair_cpu;
    let memory_slots = remaining(guest.memory_bytes, occupied.memory_bytes) / pair_memory;
    let free_permits = ceiling.saturating_sub(occupied.permits);
    let additional = cpu_slots.min(memory_slots).min(u64::from(free_permits));
    let additional = u32::try_from(additional).map_err(|_| capacity_error("docker capacity"))?;
    let possible = occupied
        .permits
        .checked_add(additional)
        .ok_or_else(|| capacity_error("docker capacity"))?;
    Ok(JobCapacity {
        total: possible.min(ceiling),
        pair_fits,
    })
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
