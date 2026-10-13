//! Durable launch rows own capacity until their cleanup proof commits.

use crate::IntentState;
use crate::journal::Journal;
use crate::reconcile::IntentRow;
use crate::scale_set::EnsureError;
use crate::stage::PairEngine;

/// SQL predicate matching [`holds`] for bounded capacity reads.
pub(crate) const HOLDS_ROWS_SQL: &str = concat!(
    "kind = 'launch' AND cleanup_proven = 0 AND ",
    "(state != 'failed' OR docker_id IS NOT NULL OR dind_id IS NOT NULL ",
    "OR worker_volume IS NOT NULL OR github_runner_id IS NOT NULL)"
);

pub(super) async fn busy_except<E: PairEngine + ?Sized>(
    journal: &Journal,
    engine: &E,
    capacity: u32,
    except: Option<&str>,
) -> Result<bool, EnsureError> {
    if occupied_except(journal, except).await? >= capacity {
        return Ok(true);
    }
    Ok(running_count(journal, engine).await? >= capacity)
}

pub(super) async fn occupied(journal: &Journal) -> Result<u32, EnsureError> {
    occupied_except(journal, None).await
}

/// Occupied permits, excluding one idless uncertain subject.
///
/// That subject is the redelivered `m{message_id}r{request_id}`. Its empty row
/// must not block its own mint at admission. A docker id, a dind id, or a
/// worker volume still holds the permit.
pub(super) async fn occupied_except(
    journal: &Journal,
    except: Option<&str>,
) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let count = rows
        .iter()
        .filter(|row| holds(row) && !idless_self(row, except))
        .count();
    u32::try_from(count).map_err(|_| EnsureError::Unexpected {
        status: 0,
        step: "capacity",
    })
}

fn idless_self(row: &IntentRow, except: Option<&str>) -> bool {
    let Some(subject) = except else {
        return false;
    };
    // An attempted acquire or JIT may have applied its effect. That row keeps
    // its reservation even without worker ids; only a never-attempted empty
    // row is safe to except for its own redelivered mint.
    row.subject == subject && idless_unattempted(row)
}

pub(super) fn idless_unattempted(row: &IntentRow) -> bool {
    row.state == IntentState::Uncertain
        && !row.acquire_attempted
        && !row.jit_requested
        && row.docker_id.as_deref().is_none_or(str::is_empty)
        && row.dind_id.as_deref().is_none_or(str::is_empty)
        && row.worker_volume.as_deref().is_none_or(str::is_empty)
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

/// Recover and clean exact worker resources after the runner exits.
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
