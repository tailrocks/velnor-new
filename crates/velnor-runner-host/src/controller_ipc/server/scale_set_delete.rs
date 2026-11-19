// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::{Journal, ScaleSetDeleteGate};

use super::super::wire::{Request, Response, ScaleSetDeleteReply, ScaleSetDeleteStatus};
use super::{ControlState, configured_exactly, transport::read_drain_token};

pub(super) async fn handle(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    request: Request,
) -> Result<Response, HostError> {
    let status = match request {
        Request::ScaleSetDeleteStart {
            binding,
            token,
            nonce,
        } => start(journal, state, state_path, &binding, &token, &nonce).await?,
        Request::ScaleSetDeleteStatus {
            connection_id,
            nonce,
        } => state.scale_set_delete_status(connection_id.as_str(), &nonce)?,
        _ => return Err(HostError::Lock),
    };
    Ok(Response {
        scale_set_delete: Some(ScaleSetDeleteReply { status }),
        ..Response::default()
    })
}

async fn start(
    journal: &Journal,
    state: &ControlState,
    state_path: &Path,
    binding: &RepositoryConnection,
    token: &str,
    nonce: &str,
) -> Result<ScaleSetDeleteStatus, HostError> {
    let marker = state_path
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    if read_drain_token(&marker).as_deref() != Some(token)
        || !configured_exactly(state, binding)?
        || !state.admission_ready()
    {
        return Ok(ScaleSetDeleteStatus::Stale);
    }
    let scope = journal.existing_connection_scope(binding).await?;
    Ok(match scope.scale_set_delete_gate().await? {
        ScaleSetDeleteGate::NotOwned => ScaleSetDeleteStatus::NotOwned,
        ScaleSetDeleteGate::Deleted => ScaleSetDeleteStatus::Deleted,
        ScaleSetDeleteGate::NotDrained => ScaleSetDeleteStatus::Stale,
        ScaleSetDeleteGate::Owned(_) => {
            state.queue_scale_set_delete(binding.connection_id.as_str(), nonce)?
        }
    })
}

#[cfg(test)]
#[path = "scale_set_delete/tests.rs"]
mod tests;
