use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Instant;

use rustix::process::{Pid, Signal, kill_process_group};
use velnor_runner_github::EncodedJit;

use super::machine::failure;
use super::store::{ACL_BUDGET, Store};
use super::{GUEST_RUNNER, JobPaths, PID_NAMESPACE, ReservationId, RustRunnerLauncher};
use crate::error::{HostError, RustRunnerFailure};

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: tokio::process::Child,
    store: PathBuf,
    jit_name: String,
    binary: String,
}

impl RustRunnerLauncher {
    pub fn start_job(
        &self,
        reservation: ReservationId,
        jit: &EncodedJit,
    ) -> Result<RunningJob, HostError> {
        let paths = JobPaths::of(reservation);
        let store = self.store()?;
        store.require_runner_binary(Instant::now() + ACL_BUDGET)?;
        let binary = store.runner_digest()?;
        store.write_jit(paths.jit_name(), jit.expose().as_bytes())?;
        match self.spawn_runner(&paths) {
            Ok(child) => Ok(RunningJob {
                child,
                store: self.host_store.clone(),
                jit_name: paths.jit_name().to_owned(),
                binary,
            }),
            Err(error) => {
                let _ = store.remove_jit(paths.jit_name());
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

impl RunningJob {
    pub(super) fn binary(&self) -> &str {
        &self.binary
    }

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
        let _ = Store::open(&self.store).and_then(|store| store.remove_jit(&self.jit_name));
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        self.remove_jit_file();
    }
}
