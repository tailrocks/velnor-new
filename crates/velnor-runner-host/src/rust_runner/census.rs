// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::host_toml_sha256;
use crate::config::{ConnectionId, RepositoryConnection};
use crate::error::HostError;

const FILE: &str = "rust-runner-census.json";
const MAX_BYTES: u64 = 65_536;
const FRESH: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_millis(100);
const TOKEN_LENGTH: usize = 32;
const TOKEN_READ: u64 = 65;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tally {
    pub(crate) id: String,
    pub(crate) slots: u32,
    pub(crate) drain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Census {
    pub(crate) config_sha256: String,
    pub(crate) updated_unix_ms: u64,
    pub(crate) drain: Option<String>,
    pub(crate) connections: Vec<Tally>,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or_default()
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
    let mut token = String::new();
    file.take(TOKEN_READ).read_to_string(&mut token).ok()?;
    let valid = token.len() == TOKEN_LENGTH
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    valid.then_some(token)
}

pub(crate) fn connection_marker(state: &Path, id: &str) -> std::path::PathBuf {
    state.join("connection-drain").join(id)
}

pub(crate) fn publish(
    state: &Path,
    config_sha256: &str,
    held: Vec<(String, u32)>,
) -> Result<(), HostError> {
    let census = Census {
        config_sha256: config_sha256.to_owned(),
        updated_unix_ms: now_unix_ms(),
        drain: marker_token(&state.join("drain")),
        connections: held
            .into_iter()
            .map(|(id, slots)| Tally {
                drain: marker_token(&connection_marker(state, &id)),
                id,
                slots,
            })
            .collect(),
    };
    let text = serde_json::to_vec(&census).map_err(|_| HostError::Path)?;
    let staging = state.join(format!(".{FILE}.tmp"));
    std::fs::write(&staging, text).map_err(|_| HostError::Path)?;
    std::fs::rename(&staging, state.join(FILE)).map_err(|_| HostError::Path)
}

fn read(state: &Path) -> Option<Census> {
    let file = std::fs::File::open(state.join(FILE)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    let census: Census = serde_json::from_slice(&bytes).ok()?;
    let age = now_unix_ms().saturating_sub(census.updated_unix_ms);
    (age <= u64::try_from(FRESH.as_millis()).unwrap_or(u64::MAX)).then_some(census)
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
        let drained = read(state).is_some_and(|census| {
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
        let drained = read(state).is_some_and(|census| {
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
            census.config_sha256 == current
                && census
                    .connections
                    .iter()
                    .all(|tally| tally.id != binding.connection_id.as_str())
        });
        Some(released)
    })
}
