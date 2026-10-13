//! Durable launch rows own capacity until their cleanup proof commits.

use crate::IntentState;
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::PairEngine;

pub(super) async fn busy<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
    capacity: u32,
) -> Result<bool, EnsureError> {
    if occupied(journal).await? >= capacity {
        return Ok(true);
    }
    Ok(running_count(journal, engine).await? >= capacity)
}

pub(super) async fn occupied(journal: &Journal) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let count = rows.iter().filter(|row| holds(row)).count();
    u32::try_from(count).map_err(|_| EnsureError::Unexpected {
        status: 0,
        step: "capacity",
    })
}

pub(super) async fn running_count<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut count = 0u32;
    for row in &rows {
        let Some(id) = row.docker_id.as_deref().filter(|_| holds(row)) else {
            continue;
        };
        if engine.running(id).await.map_err(map_docker)? {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// Keep exited workers occupied until a durable completion path proves cleanup.
///
/// Local container exit does not prove the official runner is absent. The
/// completion reconciler owns container and volume removal after that proof.
pub(super) fn holds(row: &IntentRow) -> bool {
    row.kind == "launch"
        && !row.cleanup_proven
        && (row.state != IntentState::Failed
            || row.docker_id.is_some()
            || row.dind_id.is_some()
            || row.worker_volume.is_some()
            || row.github_runner_id.is_some())
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

fn map_docker(error: crate::error::HostError) -> EnsureError {
    match error {
        crate::error::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "docker",
        },
    }
}
