// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ExitStatus, Stdio};

use rustix::process::{Pid, Signal, kill_process_group};

use super::machine::failure;
use super::{GUEST_RUNNER, GUEST_STORE, PID_NAMESPACE, RustRunnerLauncher};
use crate::error::{HostError, RustRunnerFailure};

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: Child,
    jit_file: PathBuf,
}

impl RustRunnerLauncher {
    pub fn start_job(
        &self,
        reservation: i64,
        encoded_jit_config: &str,
    ) -> Result<RunningJob, HostError> {
        let jit_file = self.write_jit_file(reservation, encoded_jit_config)?;
        match self.spawn_runner(reservation) {
            Ok(child) => Ok(RunningJob { child, jit_file }),
            Err(error) => {
                let _ = std::fs::remove_file(&jit_file);
                Err(error)
            }
        }
    }

    fn spawn_runner(&self, reservation: i64) -> Result<Child, HostError> {
        self.in_machine()
            .args(PID_NAMESPACE)
            .arg(GUEST_RUNNER)
            .args(["run", "--jit"])
            .arg(format!("@{GUEST_STORE}/jit/{reservation}.jit"))
            .stdin(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|_| failure(RustRunnerFailure::Spawn))
    }

    fn write_jit_file(&self, reservation: i64, encoded: &str) -> Result<PathBuf, HostError> {
        let directory = self.host_store.join("jit");
        let file = directory.join(format!("{reservation}.jit"));
        let staging = directory.join(format!(".{reservation}.jit.tmp"));
        let write = || -> std::io::Result<()> {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&directory)?;
            match std::fs::remove_file(&staging) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
            let mut staged = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&staging)?;
            staged.write_all(encoded.as_bytes())?;
            staged.sync_all()?;
            std::fs::rename(&staging, &file)
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&staging);
            failure(RustRunnerFailure::JitFile)
        })?;
        Ok(file)
    }
}

impl RunningJob {
    pub fn wait(mut self) -> Result<ExitStatus, HostError> {
        let status = self.child.wait();
        self.remove_jit_file();
        status.map_err(|_| failure(RustRunnerFailure::Wait))
    }

    pub fn stop(mut self) -> Result<(), HostError> {
        self.kill_and_reap()
    }

    fn kill_and_reap(&mut self) -> Result<(), HostError> {
        if let Ok(pid) = i32::try_from(self.child.id())
            && let Some(group) = Pid::from_raw(pid)
        {
            let _ = kill_process_group(group, Signal::KILL);
        }
        let reaped = self.child.wait();
        self.remove_jit_file();
        reaped
            .map(drop)
            .map_err(|_| failure(RustRunnerFailure::Wait))
    }

    fn remove_jit_file(&self) {
        let _ = std::fs::remove_file(&self.jit_file);
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.kill_and_reap();
        } else {
            self.remove_jit_file();
        }
    }
}
