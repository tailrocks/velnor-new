//! A launch row occupies one slot until cleanup is proven.
//! Busy means occupancy or the running count has reached capacity.

use velnor_runner_host::IntentState;
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::stage::PairEngine;
use velnor_runner_journal::journal::Journal;
use velnor_runner_journal::reconcile::IntentRow;

/// Busy means occupancy or the running count has reached capacity.
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
pub async fn occupied(journal: &Journal) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let count = rows.iter().filter(|row| holds(row)).count();
    u32::try_from(count).map_err(|_| EnsureError::Unexpected {
        status: 0,
        step: "capacity",
    })
}

/// Count held rows whose recorded worker is still running.
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

/// Recover and clean exact worker resources after the runner exits.
pub async fn release_exited<E: PairEngine + ?Sized>(
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
    // Pending and uncertain rows can represent an acquire or JIT request that
    // reached GitHub before the process stopped. Local Docker absence cannot
    // settle that remote effect, so keep both its reservation and owned files.
    if matches!(row.state, IntentState::Pending | IntentState::Uncertain) {
        return Ok(());
    }
    let Some(volume) = row.worker_volume.as_deref() else {
        return Ok(());
    };
    let (runner, dind) = recover_pair(journal, engine, row).await?;
    if let Some(runner) = runner.as_deref()
        && (engine.running(runner).await.map_err(map_docker)?
            || !delete_owned(engine, runner, volume, "runner").await?)
    {
        return Ok(());
    }
    if let Some(dind) = dind.as_deref()
        && !delete_owned(engine, dind, volume, "dind").await?
    {
        return Ok(());
    }
    if !engine
        .remove_worker_volumes(volume)
        .await
        .map_err(map_docker)?
    {
        return Ok(());
    }
    journal.record_cleanup(row.id).await.map_err(map_journal)?;
    Ok(())
}

async fn recover_pair<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
    row: &IntentRow,
) -> Result<(Option<String>, Option<String>), EnsureError> {
    let Some(volume) = row.worker_volume.as_deref() else {
        return Ok((row.docker_id.clone(), row.dind_id.clone()));
    };
    let runner = recover_id(journal, engine, row, volume, "runner").await?;
    let dind = recover_id(journal, engine, row, volume, "dind").await?;
    Ok((runner, dind))
}

async fn recover_id<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
    row: &IntentRow,
    volume: &str,
    role: &str,
) -> Result<Option<String>, EnsureError> {
    let recorded = match role {
        "runner" => row.docker_id.as_deref(),
        "dind" => row.dind_id.as_deref(),
        _ => {
            return Err(EnsureError::Unexpected {
                status: 0,
                step: "docker",
            });
        }
    };
    let name = format!("{volume}-{role}");
    let observed = engine
        .worker_id_for_name(&name, volume, role)
        .await
        .map_err(map_docker)?;
    if let Some(id) = recorded {
        return match observed {
            Some(found) if found == id => Ok(Some(id.to_owned())),
            Some(_) => Err(EnsureError::Unexpected {
                status: 0,
                step: "docker ownership",
            }),
            None => match engine.id_for_name(id).await.map_err(map_docker)? {
                None => Ok(None),
                Some(_) => Err(EnsureError::Unexpected {
                    status: 0,
                    step: "docker ownership",
                }),
            },
        };
    }
    let Some(id) = observed else {
        return Ok(None);
    };
    let (runner, dind) = if role == "runner" {
        (Some(id.as_str()), None)
    } else {
        (None, Some(id.as_str()))
    };
    journal
        .bind_worker(row.id, runner, dind)
        .await
        .map_err(map_journal)?;
    Ok(Some(id))
}

async fn delete_owned<E: PairEngine + ?Sized>(
    engine: &E,
    id: &str,
    volume: &str,
    role: &str,
) -> Result<bool, EnsureError> {
    let name = format!("{volume}-{role}");
    match engine
        .worker_id_for_name(&name, volume, role)
        .await
        .map_err(map_docker)?
    {
        None => match engine.id_for_name(id).await.map_err(map_docker)? {
            None => Ok(true),
            Some(_) => Ok(false),
        },
        Some(found) if found == id => {
            engine.remove(id).await.map_err(map_docker)?;
            Ok(engine
                .worker_id_for_name(&name, volume, role)
                .await
                .map_err(map_docker)?
                .is_none())
        }
        Some(_) => Ok(false),
    }
}

/// A launch row holds its slot until cleanup is proven or it failed.
pub fn holds(row: &IntentRow) -> bool {
    row.kind == "launch" && !row.cleanup_proven && row.state != IntentState::Failed
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
