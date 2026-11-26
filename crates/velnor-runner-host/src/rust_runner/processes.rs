// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Stdio;

use super::{GUEST_RUNNER, RustRunnerLauncher};
use crate::error::{HostError, RustRunnerFailure};

#[cfg(test)]
mod tests;

const WORK_DIR_PREFIX: &str = "--work-dir=";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineProcess {
    pub pid: u32,
    pub args: String,
}

impl MachineProcess {
    #[must_use]
    pub fn names_work_dir(&self, work_dir: &str) -> bool {
        names_work_dir(&self.args, work_dir)
    }

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

pub(crate) fn names_work_dir(args: &str, work_dir: &str) -> bool {
    args.split_whitespace().any(|token| {
        let path = token.strip_prefix(WORK_DIR_PREFIX).unwrap_or(token);
        path.strip_prefix(work_dir)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

impl RustRunnerLauncher {
    pub fn list_processes(&self) -> Result<Vec<MachineProcess>, HostError> {
        let output = self
            .as_root()
            .args(["ps", "-eo", "pid=,args="])
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .map_err(|_| HostError::RustRunner(RustRunnerFailure::OrbUnavailable))?;
        if !output.status.success() {
            return Err(HostError::RustRunner(RustRunnerFailure::ProcessList));
        }
        let text = String::from_utf8(output.stdout)
            .map_err(|_| HostError::RustRunner(RustRunnerFailure::ProcessList))?;
        parse_ps(&text).map_err(HostError::RustRunner)
    }

    pub fn kill_processes(&self, pids: &[u32]) -> Result<(), HostError> {
        if pids.is_empty() {
            return Ok(());
        }
        let status = self
            .as_root()
            .args(["kill", "-9"])
            .args(pids.iter().map(u32::to_string))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|_| HostError::RustRunner(RustRunnerFailure::OrbUnavailable))?;
        if status.success() {
            Ok(())
        } else {
            Err(HostError::RustRunner(RustRunnerFailure::ProcessKill))
        }
    }
}
