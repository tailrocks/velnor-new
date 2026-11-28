// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::controller::{fresh, now_unix_ms};
use super::host_toml_sha256;
use crate::config::{ConnectionId, RepositoryConnection};
use crate::error::HostError;

const FILE: &str = "rust-runner-census.json";
const MAX_BYTES: u64 = 65_536;
const RETRY: Duration = Duration::from_millis(100);
const TOKEN_LENGTH: usize = 32;
const TOKEN_READ: u64 = 65;

pub(crate) struct Held {
    pub(crate) id: String,
    pub(crate) slots: u32,
    pub(crate) admitting: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tally {
    pub(crate) id: String,
    pub(crate) slots: u32,
    pub(crate) drain: Option<String>,
    pub(crate) request: Option<String>,
    pub(crate) admitting: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Census {
    pub(crate) config_sha256: String,
    pub(crate) updated_unix_ms: u64,
    pub(crate) drain: Option<String>,
    pub(crate) reconciled: bool,
    pub(crate) connections: Vec<Tally>,
}

#[must_use]
pub fn parse_marker_token(bytes: &[u8]) -> Option<&str> {
    if bytes.len() != TOKEN_LENGTH
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

pub(crate) fn marker_token(path: &Path) -> Option<String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(TOKEN_READ).read_to_end(&mut bytes).ok()?;
    parse_marker_token(&bytes).map(str::to_owned)
}

pub(crate) fn connection_marker(state: &Path, id: &str) -> std::path::PathBuf {
    state.join("connection-drain").join(id)
}

fn request_marker(state: &Path, id: &str) -> std::path::PathBuf {
    state.join("census-request").join(id)
}

pub(crate) fn marker_absent(path: &Path) -> bool {
    matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == ErrorKind::NotFound)
}

pub(crate) fn publish_idle(state: &Path, config_sha256: &str) -> Result<(), HostError> {
    write(state, config_sha256, Vec::new(), false)
}

pub(crate) fn publish(state: &Path, config_sha256: &str, held: Vec<Held>) -> Result<(), HostError> {
    write(state, config_sha256, held, true)
}

fn write(
    state: &Path,
    config_sha256: &str,
    held: Vec<Held>,
    reconciled: bool,
) -> Result<(), HostError> {
    let census = Census {
        config_sha256: config_sha256.to_owned(),
        updated_unix_ms: now_unix_ms(),
        drain: marker_token(&state.join("drain")),
        reconciled,
        connections: held
            .into_iter()
            .map(|held| Tally {
                drain: marker_token(&connection_marker(state, &held.id)),
                request: marker_token(&request_marker(state, &held.id)),
                id: held.id,
                slots: held.slots,
                admitting: held.admitting,
            })
            .collect(),
    };
    let text = serde_json::to_vec(&census).map_err(|_| HostError::Path)?;
    super::atomic::write(&state.join(FILE), &text)
}

fn read(state: &Path) -> Option<Census> {
    let file = std::fs::File::open(state.join(FILE)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    let census: Census = serde_json::from_slice(&bytes).ok()?;
    fresh(census.updated_unix_ms, now_unix_ms()).then_some(census)
}

fn read_reconciled(state: &Path) -> Option<Census> {
    read(state).filter(|census| census.reconciled)
}

fn wait_until(timeout: Duration, mut proven: impl FnMut() -> Option<bool>) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match proven() {
            Some(true) => return true,
            None => return false,
            Some(false) => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        thread::sleep(RETRY.min(left));
    }
}

pub fn wait_for_rust_runner_drain(state: &Path, timeout: Duration) -> Result<bool, HostError> {
    let marker = state.join("drain");
    let Some(token) = marker_token(&marker) else {
        return Ok(false);
    };
    Ok(wait_until(timeout, || {
        if marker_token(&marker).as_deref() != Some(token.as_str()) {
            return None;
        }
        let drained = read_reconciled(state).is_some_and(|census| {
            census.drain.as_deref() == Some(token.as_str())
                && census.connections.iter().all(|tally| tally.slots == 0)
        });
        Some(drained)
    }))
}

pub fn wait_for_rust_runner_connection_drain(
    state: &Path,
    connection_id: &ConnectionId,
    timeout: Duration,
) -> Result<bool, HostError> {
    let marker = connection_marker(state, connection_id.as_str());
    let Some(token) = marker_token(&marker) else {
        return Ok(false);
    };
    Ok(wait_until(timeout, || {
        if marker_token(&marker).as_deref() != Some(token.as_str()) {
            return None;
        }
        let drained = read_reconciled(state).is_some_and(|census| {
            census.connections.iter().any(|tally| {
                tally.id == connection_id.as_str()
                    && tally.slots == 0
                    && tally.drain.as_deref() == Some(token.as_str())
            })
        });
        Some(drained)
    }))
}

pub(super) fn wait_until_unserved(
    state: &Path,
    binding: &RepositoryConnection,
    timeout: Duration,
) -> bool {
    let marker = connection_marker(state, binding.connection_id.as_str());
    let Some(token) = marker_token(&marker) else {
        return false;
    };
    wait_until(timeout, || {
        if marker_token(&marker).as_deref() != Some(token.as_str()) {
            return None;
        }
        let Some(current) = host_toml_sha256(state) else {
            return Some(false);
        };
        let released = read(state).is_some_and(|census| {
            census.reconciled
                && census.config_sha256 == current
                && census
                    .connections
                    .iter()
                    .all(|tally| tally.id != binding.connection_id.as_str())
        });
        Some(released)
    })
}

pub fn wait_for_rust_runner_connection_admitted(
    state: &Path,
    connection_id: &ConnectionId,
    timeout: Duration,
) -> Result<bool, HostError> {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let request = request_marker(state, connection_id.as_str());
    let directory = request.parent().ok_or(HostError::Path)?;
    std::fs::create_dir_all(directory).map_err(|_| HostError::Path)?;
    super::atomic::write(&request, nonce.as_bytes())?;
    Ok(wait_until(timeout, || {
        let Some(current) = host_toml_sha256(state) else {
            return Some(false);
        };
        let admitted = read(state).is_some_and(|census| {
            census.config_sha256 == current
                && census.connections.iter().any(|tally| {
                    tally.id == connection_id.as_str()
                        && tally.admitting
                        && tally.drain.is_none()
                        && tally.request.as_deref() == Some(nonce.as_str())
                })
        });
        Some(admitted)
    }))
}

#[cfg(test)]
mod tests;
