// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::machine::failure;
use super::store::Store;
use super::{MachineProcess, ReservationId, RustRunnerLauncher};
use crate::error::{HostError, RustRunnerFailure};

impl RustRunnerLauncher {
    pub fn sweep(&self) -> Result<Vec<ReservationId>, HostError> {
        let runners = self.runner_pids()?;
        if !runners.is_empty() {
            self.kill_processes(&runners)?;
            if !self.runner_pids()?.is_empty() {
                return Err(failure(RustRunnerFailure::ProcessesSurvive));
            }
        }
        Store::open(&self.host_store)?.remove_obsolete()?;
        self.sweep_job_dirs()
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
