// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{JIT_DIRECTORY, RustRunnerLauncher, heartbeat_readiness, host_toml_sha256};
use crate::error::{HostError, RustRunnerFailure};
use crate::readiness::Readiness;

const PRIVATE_MODE: u32 = 0o700;

impl RustRunnerLauncher {
    pub fn probe(&self, timeout: Duration) -> Result<(), HostError> {
        let deadline = Instant::now() + timeout;
        let within = |limit: Instant| limit.saturating_duration_since(Instant::now());
        if !self
            .clone()
            .with_orb_deadline(within(deadline))
            .machine_exists()?
        {
            return Err(HostError::RustRunner(RustRunnerFailure::MachineMissing));
        }
        self.clone()
            .with_orb_deadline(within(deadline))
            .require_runner_binary()?;
        self.store()?.require_runner_binary(deadline)?;
        let metadata = std::fs::symlink_metadata(self.host_store.join(JIT_DIRECTORY))
            .map_err(|_| HostError::RustRunner(RustRunnerFailure::HostStore))?;
        if !metadata.is_dir() {
            return Err(HostError::RustRunner(RustRunnerFailure::HostStore));
        }
        if metadata.permissions().mode() & 0o777 != PRIVATE_MODE {
            return Err(HostError::RustRunner(
                RustRunnerFailure::JitDirectoryAccessible,
            ));
        }
        Ok(())
    }
}

pub fn run_rust_runner_doctor_probe(
    state: &Path,
    launcher: &RustRunnerLauncher,
    timeout: Duration,
) -> Result<serde_json::Value, HostError> {
    launcher.probe(timeout)?;
    let fresh = host_toml_sha256(state)
        .map(|sha256| heartbeat_readiness(state, &sha256))
        .filter(|readiness| *readiness == Readiness::Ready);
    let Some(readiness) = fresh else {
        return Err(HostError::RustRunner(RustRunnerFailure::HeartbeatNotFresh));
    };
    Ok(serde_json::json!({
        "state": "completed",
        "report": {
            "machine": "reachable",
            "runner_binary": "installed",
            "jit_directory": "private",
            "heartbeat": "fresh",
        },
        "readiness": readiness,
    }))
}
