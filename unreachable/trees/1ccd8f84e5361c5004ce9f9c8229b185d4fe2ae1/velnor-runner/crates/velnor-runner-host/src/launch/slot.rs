//! A running launch container occupies one slot.
//! Busy means the running count has reached capacity.

use bollard::Docker;

use crate::IntentState;
use crate::journal::Journal;
use crate::scale_set::EnsureError;

pub(super) async fn busy(
    journal: &Journal,
    docker: &Docker,
    capacity: u32,
) -> Result<bool, EnsureError> {
    Ok(running_count(journal, docker).await? >= capacity)
}

pub(super) async fn running_count(journal: &Journal, docker: &Docker) -> Result<u32, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let mut count = 0u32;
    for row in rows {
        if row.kind != "launch" {
            continue;
        }
        let running = observed(row.state, row.docker_id.as_deref(), docker).await;
        if occupies(row.state, row.docker_id.as_deref(), running) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

async fn observed(state: IntentState, docker_id: Option<&str>, docker: &Docker) -> bool {
    if state == IntentState::Failed {
        return false;
    }
    let Some(id) = docker_id else {
        return false;
    };
    running(docker, id).await
}

/// Failed rows, a missing docker id, and a stopped container do not occupy a slot.
/// `running` is false when inspect fails.
#[must_use]
pub(crate) const fn occupies(state: IntentState, docker_id: Option<&str>, running: bool) -> bool {
    !matches!(state, IntentState::Failed) && docker_id.is_some() && running
}

async fn running(docker: &Docker, id: &str) -> bool {
    let Ok(info) = docker.inspect_container(id, None).await else {
        return false;
    };
    info.state.and_then(|state| state.running).unwrap_or(false)
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
