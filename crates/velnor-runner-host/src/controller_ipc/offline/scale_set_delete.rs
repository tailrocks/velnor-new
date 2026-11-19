// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::config::RepositoryConnection;
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::journal::ScaleSetDeleteGate;

use super::super::client::CreatedSetDeletion;
use super::super::server::read_drain_token;
use super::{open_existing_journal, regular_file_exists, runtime};

pub(in super::super) fn scale_set_delete(
    state: &Path,
    binding: &RepositoryConnection,
    token: &str,
) -> Result<CreatedSetDeletion, HostError> {
    let _daemon = DaemonLock::try_acquire_daemon(state)?;
    let _admission = DaemonLock::try_acquire(&state.join("admission.lock"))?;
    let marker = state
        .join("connection-drain")
        .join(binding.connection_id.as_str());
    if read_drain_token(&marker).as_deref() != Some(token) {
        return Ok(CreatedSetDeletion::Uncertain);
    }
    let journal_path = state.join("launch.db");
    if !regular_file_exists(&journal_path)? {
        return Ok(CreatedSetDeletion::NotOwned);
    }
    let (config, journal, _lineage) = open_existing_journal(state, &journal_path)?;
    if config.connection(&binding.connection_id) != Some(binding) {
        return Ok(CreatedSetDeletion::Uncertain);
    }
    let gate = runtime()?.block_on(async {
        journal
            .existing_connection_scope(binding)
            .await?
            .scale_set_delete_gate()
            .await
    })?;
    Ok(match gate {
        ScaleSetDeleteGate::NotOwned => CreatedSetDeletion::NotOwned,
        ScaleSetDeleteGate::Deleted => CreatedSetDeletion::Deleted,
        ScaleSetDeleteGate::NotDrained => CreatedSetDeletion::Uncertain,
        ScaleSetDeleteGate::Owned(_) => CreatedSetDeletion::ControllerOffline,
    })
}

#[cfg(test)]
#[path = "scale_set_delete_tests.rs"]
mod tests;
