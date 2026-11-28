// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::HostError;
use crate::readiness::Readiness;

const FILE: &str = "rust-runner-controller.json";
const MAX_BYTES: u64 = 4096;
const FRESH: Duration = Duration::from_secs(30);
const AHEAD: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Startup,
    WaitingForCredentials,
    MachineUnavailable,
    RunnerFailing,
    SessionFailing,
    Ready,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    config_sha256: String,
    phase: Phase,
    updated_unix_ms: u64,
}

pub(in crate::rust_runner) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or_default()
}

pub(crate) fn write(state: &Path, config_sha256: &str, phase: Phase) -> Result<(), HostError> {
    let record = Record {
        config_sha256: config_sha256.to_owned(),
        phase,
        updated_unix_ms: now_unix_ms(),
    };
    let text = serde_json::to_vec(&record).map_err(|_| HostError::Path)?;
    let staging = state.join(format!(".{FILE}.tmp"));
    std::fs::write(&staging, text).map_err(|_| HostError::Path)?;
    std::fs::rename(&staging, state.join(FILE)).map_err(|_| HostError::Path)
}

fn read(state: &Path) -> Option<Record> {
    let file = std::fs::File::open(state.join(FILE)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(in crate::rust_runner) fn fresh(updated_unix_ms: u64, now_unix_ms: u64) -> bool {
    let millis = |limit: Duration| u64::try_from(limit.as_millis()).unwrap_or(u64::MAX);
    now_unix_ms.saturating_sub(updated_unix_ms) <= millis(FRESH)
        && updated_unix_ms <= now_unix_ms.saturating_add(millis(AHEAD))
}

pub(crate) fn assess(state: &Path, config_sha256: &str) -> Readiness {
    let Some(record) = read(state) else {
        return Readiness::Degraded;
    };
    if !fresh(record.updated_unix_ms, now_unix_ms()) {
        return Readiness::Degraded;
    }
    match record.phase {
        Phase::WaitingForCredentials => Readiness::WaitingForCredentials,
        Phase::Ready if record.config_sha256 == config_sha256 => Readiness::Ready,
        Phase::Startup | Phase::Ready => Readiness::Reconciling,
        Phase::MachineUnavailable | Phase::RunnerFailing | Phase::SessionFailing => {
            Readiness::Degraded
        }
    }
}
