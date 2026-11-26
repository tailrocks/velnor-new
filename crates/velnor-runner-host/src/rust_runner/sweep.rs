// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::machine::failure;
use super::{JIT_DIRECTORY, MachineProcess, RustRunnerLauncher};
use crate::error::{HostError, RustRunnerFailure};

impl RustRunnerLauncher {
    pub fn sweep(&self) -> Result<(), HostError> {
        let runners = self.runner_pids()?;
        if !runners.is_empty() {
            self.kill_processes(&runners)?;
            if !self.runner_pids()?.is_empty() {
                return Err(failure(RustRunnerFailure::ProcessesSurvive));
            }
        }
        for name in [JIT_DIRECTORY, "work"] {
            empty_directory(&self.host_store.join(name))?;
        }
        Ok(())
    }

    fn runner_pids(&self) -> Result<Vec<u32>, HostError> {
        Ok(self
            .list_processes()?
            .iter()
            .filter(|process| process.is_runner())
            .map(|process: &MachineProcess| process.pid)
            .collect())
    }
}

fn empty_directory(directory: &Path) -> Result<(), HostError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(failure(RustRunnerFailure::HostStore)),
    };
    for entry in entries {
        let entry = entry.map_err(|_| failure(RustRunnerFailure::HostStore))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|_| failure(RustRunnerFailure::HostStore))?;
        let removed = if kind.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.map_err(|_| failure(RustRunnerFailure::HostStore))?;
    }
    Ok(())
}
