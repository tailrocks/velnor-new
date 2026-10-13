//! A live runner container still occupies the one slot.

use bollard::Docker;

use crate::IntentState;
use crate::journal::Journal;
use crate::scale_set::EnsureError;

pub(super) async fn busy(journal: &Journal, docker: &Docker) -> Result<bool, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    for row in rows {
        if row.kind != "launch" || row.state == IntentState::Failed {
            continue;
        }
        let Some(id) = row.docker_id else {
            continue;
        };
        if running(docker, &id).await {
            return Ok(true);
        }
    }
    Ok(false)
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
