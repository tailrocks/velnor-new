// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::machine::failure;
use super::{MachineProcess, RustRunnerLauncher};
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
        self.clean_all_jobs()
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
