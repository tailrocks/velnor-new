//! A launch row occupies one slot until cleanup is proven.
//! Busy means occupancy or the running count has reached capacity.

use velnor_runner_host::IntentState;
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::stage::PairEngine;
use velnor_runner_journal::journal::Journal;
use velnor_runner_journal::journal::LaunchEffectState;
use velnor_runner_journal::reconcile::IntentRow;

/// Busy means occupancy or the running count has reached capacity.
///
/// # Errors
///
/// Returns an error if the journal or Docker engine cannot report occupancy.
pub async fn busy<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
    capacity: u32,
) -> Result<bool, EnsureError> {
    if occupied(journal).await? >= capacity {
        return Ok(true);
    }
    Ok(running_count(journal, engine).await? >= capacity)
}

/// Count journal rows currently holding a launch slot.
///
/// # Errors
///
/// Returns an error if the journal cannot be read or the count exceeds `u32`.
pub async fn occupied(journal: &Journal) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let count = rows.iter().filter(|row| holds(row)).count();
    u32::try_from(count).map_err(|_| EnsureError::Unexpected {
        status: 0,
        step: "capacity",
    })
}

/// Count held rows whose recorded worker is still running.
///
/// # Errors
///
/// Returns an error if the journal or Docker engine cannot inspect a worker.
pub async fn running_count<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut count = 0u32;
    for row in &rows {
        if !holds(row) {
            continue;
        }
        let Some(id) = row.docker_id.as_deref() else {
            continue;
        };
        if engine.running(id).await.map_err(map_docker)? {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// A launch row holds its slot until cleanup is proven or it failed.
#[must_use]
pub fn holds(row: &IntentRow) -> bool {
    row.kind == "launch"
        && !row.cleanup_proven
        && !(row.state == IntentState::Failed
            && row.launch_effect == LaunchEffectState::DefiniteNoEffect)
}

fn map_journal(error: velnor_runner_host::HostError) -> EnsureError {
    match error {
        velnor_runner_host::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}

fn map_docker(error: velnor_runner_host::HostError) -> EnsureError {
    match error {
        velnor_runner_host::HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "docker",
        },
    }
}
