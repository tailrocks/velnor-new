// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{ConnectionId, RepositoryConnection};
use crate::error::HostError;

const DIRECTORY: &str = "rust-runner-scale-sets";
const MAX_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::rust_runner) enum Scope {
    Repository {
        repository_id: i64,
    },
    Organization {
        organization: String,
        runner_group: Option<String>,
    },
}

impl Scope {
    pub(in crate::rust_runner) fn of(binding: &RepositoryConnection) -> Self {
        match &binding.organization {
            None => Self::Repository {
                repository_id: binding.repository_id,
            },
            Some(organization) => Self::Organization {
                organization: organization.clone(),
                runner_group: binding.runner_group.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::rust_runner) struct Owned {
    pub(in crate::rust_runner) name: String,
    pub(in crate::rust_runner) runner_group_id: i64,
    pub(in crate::rust_runner) scope: Scope,
    pub(in crate::rust_runner) server: String,
    pub(in crate::rust_runner) scale_set_id: Option<i64>,
}

impl Owned {
    pub(in crate::rust_runner) fn intent(
        binding: &RepositoryConnection,
        runner_group_id: i64,
        server: &str,
    ) -> Self {
        Self {
            name: binding.scale_set.name().to_owned(),
            runner_group_id,
            scope: Scope::of(binding),
            server: server.to_owned(),
            scale_set_id: None,
        }
    }

    fn describes(&self, other: &Self) -> bool {
        self.name == other.name
            && self.runner_group_id == other.runner_group_id
            && self.scope == other.scope
            && self.server == other.server
    }
}

fn record(state: &Path, connection: &ConnectionId) -> PathBuf {
    state.join(DIRECTORY).join(connection.as_str())
}

pub(in crate::rust_runner) fn read(
    state: &Path,
    connection: &ConnectionId,
) -> Result<Option<Owned>, HostError> {
    let file = match std::fs::File::open(record(state, connection)) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(HostError::Config),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| HostError::Config)
}

pub(in crate::rust_runner) fn bound(
    state: &Path,
    connection: &ConnectionId,
) -> Result<Option<i64>, HostError> {
    Ok(read(state, connection)?.and_then(|record| record.scale_set_id))
}

pub(in crate::rust_runner) fn write(
    state: &Path,
    connection: &ConnectionId,
    owned: &Owned,
) -> Result<(), HostError> {
    let directory = state.join(DIRECTORY);
    std::fs::create_dir_all(&directory).map_err(|_| HostError::Path)?;
    let text = serde_json::to_vec(owned).map_err(|_| HostError::Path)?;
    crate::rust_runner::atomic::write(&record(state, connection), &text)
}

pub(in crate::rust_runner) fn bind(
    state: &Path,
    connection: &ConnectionId,
    intent: &Owned,
    scale_set_id: i64,
) -> Result<(), HostError> {
    match read(state, connection)? {
        Some(recorded) if recorded.scale_set_id.is_none() && recorded.describes(intent) => write(
            state,
            connection,
            &Owned {
                scale_set_id: Some(scale_set_id),
                ..recorded
            },
        ),
        Some(recorded) if recorded.scale_set_id == Some(scale_set_id) => Ok(()),
        _ => Err(HostError::Config),
    }
}

pub(in crate::rust_runner) fn adoptable(
    state: &Path,
    connection: &ConnectionId,
    intent: &Owned,
) -> Result<bool, HostError> {
    Ok(read(state, connection)?
        .is_some_and(|recorded| recorded.scale_set_id.is_none() && recorded.describes(intent)))
}

pub(in crate::rust_runner) fn drop_intent(
    state: &Path,
    connection: &ConnectionId,
) -> Result<(), HostError> {
    match read(state, connection)? {
        Some(recorded) if recorded.scale_set_id.is_none() => forget(state, connection),
        _ => Ok(()),
    }
}

pub(in crate::rust_runner) fn forget(
    state: &Path,
    connection: &ConnectionId,
) -> Result<(), HostError> {
    let path = record(state, connection);
    match std::fs::remove_file(&path) {
        Ok(()) => crate::rust_runner::atomic::sync_parent(&path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(_) => Err(HostError::Path),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::rust_runner) enum Transfer {
    Settled,
    Conflict,
}

pub(in crate::rust_runner) fn transfer(
    state: &Path,
    from: &ConnectionId,
    to: &ConnectionId,
) -> Result<Transfer, HostError> {
    let Some(moved) = read(state, from)? else {
        return Ok(Transfer::Settled);
    };
    match read(state, to)? {
        None => write(state, to, &moved)?,
        Some(existing) if existing == moved => {}
        Some(_) => return Ok(Transfer::Conflict),
    }
    forget(state, from)?;
    Ok(Transfer::Settled)
}

#[cfg(test)]
mod tests;
