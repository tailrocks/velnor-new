//! A launch row occupies one slot until cleanup is proven.
//! Busy means occupancy or the running count has reached capacity.

use crate::IntentState;
use crate::docker_spec::DeleteDecision;
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::{PairEngine, decide};

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

/// Drop exited runners. A 404 on a recorded id is proven gone. No recorded
/// runner or `DinD` id is not a live slot. A running runner stays occupied;
/// its `DinD` is removed once that runner is gone.
pub(super) async fn release_exited<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
) -> Result<(), EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    for row in &rows {
        release_row(journal, engine, row).await?;
    }
    Ok(())
}

async fn release_row<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
    row: &IntentRow,
) -> Result<(), EnsureError> {
    if !holds(row) {
        return Ok(());
    }
    let Some(runner) = row.docker_id.as_deref() else {
        if row.dind_id.is_none() {
            journal.record_cleanup(row.id).await.map_err(map_journal)?;
        }
        return Ok(());
    };
    if engine.running(runner).await.map_err(map_docker)? || !delete_recorded(engine, runner).await?
    {
        return Ok(());
    }
    if let Some(dind) = row.dind_id.as_deref()
        && !delete_recorded(engine, dind).await?
    {
        return Ok(());
    }
    journal.record_cleanup(row.id).await.map_err(map_journal)?;
    Ok(())
}

async fn delete_recorded<E: PairEngine + ?Sized>(
    engine: &E,
    id: &str,
) -> Result<bool, EnsureError> {
    match engine.id_for_name(id).await.map_err(map_docker)? {
        None => Ok(true),
        Some(found) if found == id => {
            let decision = decide(engine, id, id).await.map_err(map_docker)?;
            Ok(decision == DeleteDecision::Delete)
        }
        Some(_) => Ok(false),
    }
}

pub(super) fn holds(row: &IntentRow) -> bool {
    row.kind == "launch" && !row.cleanup_proven && row.state != IntentState::Failed
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
