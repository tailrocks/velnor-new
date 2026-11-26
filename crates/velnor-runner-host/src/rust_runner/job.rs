// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{DirBuilder, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use rustix::process::{Pid, Signal, kill_process_group};
use velnor_runner_github::EncodedJit;

use super::machine::failure;
use super::{GUEST_RUNNER, JobPaths, PID_NAMESPACE, ReservationId, RustRunnerLauncher};
use crate::error::{HostError, RustRunnerFailure};

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: tokio::process::Child,
    jit_file: PathBuf,
}

impl RustRunnerLauncher {
    pub fn start_job(
        &self,
        reservation: ReservationId,
        jit: &EncodedJit,
    ) -> Result<RunningJob, HostError> {
        let paths = JobPaths::of(&self.host_store, reservation);
        write_jit_file(paths.jit_file(), jit.expose())?;
        match self.spawn_runner(&paths) {
            Ok(child) => Ok(RunningJob {
                child,
                jit_file: paths.jit_file().to_path_buf(),
            }),
            Err(error) => {
                let _ = std::fs::remove_file(paths.jit_file());
                Err(error)
            }
        }
    }

    fn spawn_runner(&self, paths: &JobPaths) -> Result<tokio::process::Child, HostError> {
        let mut command = self.in_machine();
        command
            .args(PID_NAMESPACE)
            .arg(GUEST_RUNNER)
            .args(["run", "--jit"])
            .arg(format!("@{}", paths.guest_jit_file()))
            .args(["--work-dir", paths.guest_work_dir()])
            .stdin(Stdio::null())
            .process_group(0);
        tokio::process::Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| failure(RustRunnerFailure::Spawn))
    }
}

fn write_jit_file(file: &Path, encoded: &str) -> Result<(), HostError> {
    let (Some(directory), Some(name)) = (file.parent(), file.file_name()) else {
        return Err(failure(RustRunnerFailure::JitFile));
    };
    let staging = directory.join(format!(".{}.tmp", name.to_string_lossy()));
    let write = || -> std::io::Result<()> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
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
        std::fs::rename(&staging, file)
    };
    write().map_err(|_| {
        let _ = std::fs::remove_file(&staging);
        failure(RustRunnerFailure::JitFile)
    })
}

impl RunningJob {
    pub async fn wait(&mut self) -> Result<ExitStatus, HostError> {
        let status = self.child.wait().await;
        self.remove_jit_file();
        status.map_err(|_| failure(RustRunnerFailure::Wait))
    }

    pub async fn stop(mut self) -> Result<(), HostError> {
        if let Some(pid) = self.child.id().and_then(|id| i32::try_from(id).ok())
            && let Some(group) = Pid::from_raw(pid)
        {
            let _ = kill_process_group(group, Signal::KILL);
        }
        let reaped = self.child.wait().await;
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
        self.remove_jit_file();
    }
}
