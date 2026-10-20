//! Poll active Scale Set sessions and reconcile their event handoffs.

use std::path::Path;

use bollard::Docker;

use crate::{
    config::{HostConfig, RepositoryConnection},
    error::HostError,
    journal::Journal,
    launch::{ReleaseReadSecret, SelectedEngineContext, VerifiedWorkerImages},
};

use super::state::PreparedConnection;

#[path = "connection_poll/fairness.rs"]
pub(super) mod fairness;
#[path = "connection_poll/turn.rs"]
mod turn;

use fairness::FairnessTurn;

pub(super) struct ConnectionPollContext<'a> {
    pub(super) config: &'a HostConfig,
    pub(super) journal: &'a Journal,
    pub(super) docker: &'a Docker,
    pub(super) selected_engine: &'a SelectedEngineContext,
    pub(super) worker_images: &'a VerifiedWorkerImages,
    pub(super) state: &'a Path,
    pub(super) release_read_secret: &'a ReleaseReadSecret,
    pub(super) admission_ready: bool,
}

pub(super) async fn poll_active_connections(
    context: ConnectionPollContext<'_>,
    pending_retirements: &[RepositoryConnection],
    connections: &mut Vec<PreparedConnection>,
    mut turn: FairnessTurn,
) -> Result<ConnectionPollOutcome, HostError> {
    let mut positive_poll_settled = false;
    let mut index = 0;
    while index < connections.len() {
        let result = turn::poll_connection(
            &mut connections[index],
            &context,
            pending_retirements,
            &mut turn,
        )
        .await?;
        let Some(result) = result else {
            index += 1;
            continue;
        };
        positive_poll_settled |= result.positive_poll_settled;
        if result.remove_connection {
            connections.remove(index);
        } else {
            index += 1;
        }
    }
    Ok(ConnectionPollOutcome {
        positive_poll_settled,
        rotation_complete: turn.rotation_complete(connections.is_empty()),
    })
}

#[derive(Default)]
pub(super) struct ConnectionPollOutcome {
    pub(super) positive_poll_settled: bool,
    pub(super) rotation_complete: bool,
}
