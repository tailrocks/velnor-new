// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GUEST_RUNNER, ReservationId, RustRunnerLauncher, orb};
use crate::error::{HostError, RustRunnerFailure};

#[cfg(test)]
mod tests;

const INIT_PID: u32 = 1;
const WEDGED_EXIT: i32 = 70;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineProcess {
    pub pid: u32,
    pub args: String,
}

impl MachineProcess {
    #[must_use]
    pub fn is_runner(&self) -> bool {
        self.args
            .split_whitespace()
            .any(|token| token == GUEST_RUNNER)
    }
}

pub(crate) fn parse_ps(output: &str) -> Result<Vec<MachineProcess>, RustRunnerFailure> {
    let mut processes = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (pid, args) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(pid, args)| (pid, args.trim()));
        let pid = pid
            .parse::<u32>()
            .map_err(|_| RustRunnerFailure::ProcessList)?;
        processes.push(MachineProcess {
            pid,
            args: args.to_owned(),
        });
    }
    Ok(processes)
}

impl RustRunnerLauncher {
    pub(super) fn clean_job(&self, reservation: ReservationId) -> Result<(), HostError> {
        let mut command = self.as_root();
        command
            .arg(GUEST_RUNNER)
            .args(["sandbox-cleanup", "--reservation"])
            .arg(reservation.to_string());
        self.cleaned(command)
    }

    pub(super) fn clean_all_jobs(&self) -> Result<(), HostError> {
        let mut command = self.as_root();
        command.arg(GUEST_RUNNER).args(["sandbox-cleanup", "--all"]);
        self.cleaned(command)
    }

    fn cleaned(&self, command: std::process::Command) -> Result<(), HostError> {
        let finished = orb::run(command, self.deadline, false).map_err(HostError::RustRunner)?;
        match finished.status.code() {
            Some(0) => Ok(()),
            Some(WEDGED_EXIT) => Err(HostError::RustRunner(RustRunnerFailure::JobDirsWedged)),
            _ => Err(HostError::RustRunner(RustRunnerFailure::JobFilesLeft)),
        }
    }

    pub fn list_processes(&self) -> Result<Vec<MachineProcess>, HostError> {
        let mut command = self.as_root();
        command.args(["ps", "-eo", "pid=,args="]);
        let output = orb::run(command, self.deadline, true).map_err(HostError::RustRunner)?;
        if !output.status.success() {
            return Err(HostError::RustRunner(RustRunnerFailure::ProcessList));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| HostError::RustRunner(RustRunnerFailure::ProcessList))?;
        let processes = parse_ps(&text).map_err(HostError::RustRunner)?;
        if processes.iter().any(|process| process.pid == INIT_PID) {
            Ok(processes)
        } else {
            Err(HostError::RustRunner(RustRunnerFailure::ProcessList))
        }
    }

    pub fn kill_processes(&self, pids: &[u32]) -> Result<(), HostError> {
        if pids.is_empty() {
            return Ok(());
        }
        let mut command = self.as_root();
        command
            .args(["kill", "-9"])
            .args(pids.iter().map(u32::to_string));
        let finished = orb::run(command, self.deadline, false).map_err(HostError::RustRunner)?;
        if finished.status.success() {
            Ok(())
        } else {
            Err(HostError::RustRunner(RustRunnerFailure::ProcessKill))
        }
    }
}
