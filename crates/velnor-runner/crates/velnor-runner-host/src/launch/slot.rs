//! One launch row holds one slot until its container is confirmed absent.
//! A timeout, a bad response, or a missing id on a live row is not absence.
//! A finished row with a stopped container does not hold a slot.
//! Session rows and failed rows do not hold a slot.

use bollard::Docker;

use crate::IntentState;
use crate::journal::Journal;
use crate::scale_set::EnsureError;

use super::inspect::container_running;

/// What one inspect established for one recorded id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InspectFact {
    /// `State.Running` is true.
    Running,
    /// Confirmed stopped, or Docker returned 404.
    NotRunning,
    /// Timeout, transport error, or a body with no running bit.
    Unresolved,
    /// The row has no container id.
    NoId,
}

/// Held and running counts for one journal scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Census {
    /// Rows that still consume a permit.
    pub(crate) held: u32,
    /// Rows whose container is confirmed running.
    pub(crate) running: u32,
}

pub(super) async fn busy(
    journal: &Journal,
    docker: &Docker,
    capacity: u32,
) -> Result<bool, EnsureError> {
    Ok(census(journal, docker).await?.held >= capacity)
}

pub(super) async fn running_count(journal: &Journal, docker: &Docker) -> Result<u32, EnsureError> {
    Ok(census(journal, docker).await?.running)
}

pub(super) async fn census(journal: &Journal, docker: &Docker) -> Result<Census, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut held = 0u32;
    let mut running = 0u32;
    for row in rows {
        if row.kind != "launch" || row.cleanup_proven {
            continue;
        }
        let fact = fact_for(row.state, row.docker_id.as_deref(), docker).await;
        if slot_held(true, row.state, false, fact) {
            held = held.saturating_add(1);
        }
        if fact == InspectFact::Running {
            running = running.saturating_add(1);
        }
    }
    Ok(Census { held, running })
}

async fn fact_for(state: IntentState, docker_id: Option<&str>, docker: &Docker) -> InspectFact {
    if state == IntentState::Failed {
        return InspectFact::NotRunning;
    }
    let Some(id) = docker_id.filter(|id| !id.is_empty()) else {
        return InspectFact::NoId;
    };
    match container_running(docker, id).await {
        Ok(true) => InspectFact::Running,
        Ok(false) => InspectFact::NotRunning,
        Err(_) => InspectFact::Unresolved,
    }
}

/// True when this row consumes one permit.
///
/// `kind_launch` is false for session rows. Confirmed `NotRunning` does not hold.
/// `Unresolved` holds. `NoId` holds only while the row is pending or uncertain.
#[must_use]
pub(crate) const fn slot_held(
    kind_launch: bool,
    state: IntentState,
    cleanup_proven: bool,
    fact: InspectFact,
) -> bool {
    if !kind_launch || cleanup_proven || matches!(state, IntentState::Failed) {
        return false;
    }
    match fact {
        InspectFact::Running | InspectFact::Unresolved => true,
        InspectFact::NotRunning => false,
        InspectFact::NoId => matches!(state, IntentState::Pending | IntentState::Uncertain),
    }
}

/// Running container with an id. A missing id does not count, including pending.
#[must_use]
#[cfg(test)]
pub(crate) const fn occupies(state: IntentState, docker_id: Option<&str>, running: bool) -> bool {
    let fact = match (docker_id, running) {
        (None, _) => InspectFact::NoId,
        (Some(_), true) => InspectFact::Running,
        (Some(_), false) => InspectFact::NotRunning,
    };
    slot_held(true, state, false, fact) && running && docker_id.is_some()
}

fn map_journal(error: crate::error::HostError) -> EnsureError {
    match error {
        crate::error::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
