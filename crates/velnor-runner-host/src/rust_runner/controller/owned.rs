// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use crate::config::ConnectionId;
use crate::error::HostError;

const FILE: &str = "rust-runner-scale-sets.json";
const MAX_BYTES: u64 = 64 * 1024;

type Records = BTreeMap<String, i64>;

fn read(state: &Path) -> Result<Records, HostError> {
    let file = match std::fs::File::open(state.join(FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Records::new()),
        Err(_) => return Err(HostError::Config),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Config)?;
    if u64::try_from(bytes.len()).map_or(true, |size| size > MAX_BYTES) {
        return Err(HostError::Config);
    }
    serde_json::from_slice(&bytes).map_err(|_| HostError::Config)
}

pub(super) fn created(state: &Path, connection: &ConnectionId) -> Result<Option<i64>, HostError> {
    Ok(read(state)?.get(connection.as_str()).copied())
}

pub(super) fn record_created(
    state: &Path,
    connection: &ConnectionId,
    scale_set_id: i64,
) -> Result<(), HostError> {
    let mut records = read(state)?;
    records.insert(connection.as_str().to_owned(), scale_set_id);
    let text = serde_json::to_vec(&records).map_err(|_| HostError::Path)?;
    let staging = state.join(format!(".{FILE}.tmp"));
    std::fs::write(&staging, text).map_err(|_| HostError::Path)?;
    std::fs::rename(&staging, state.join(FILE)).map_err(|_| HostError::Path)
}
