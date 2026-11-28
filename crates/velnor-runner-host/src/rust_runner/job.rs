use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{ExitStatus, Stdio};

use rustix::process::{Pid, Signal, kill_process_group};
use velnor_runner_github::EncodedJit;
use zeroize::Zeroizing;

use super::machine::failure;
use super::{
    GUEST_RUNNER, JobPaths, PID_NAMESPACE, ReservationId, RustRunnerLauncher, runner_binary,
};
use crate::error::{HostError, RustRunnerFailure};

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: tokio::process::Child,
    binary: String,
}

impl RustRunnerLauncher {
    pub fn start_job(
        &self,
        reservation: ReservationId,
        jit: &EncodedJit,
    ) -> Result<RunningJob, HostError> {
        if !self.launch_open {
            return Err(failure(RustRunnerFailure::LaunchClosed));
        }
        runner_binary::open(&self.runner_binary)?;
        let paths = JobPaths::of(reservation);
        let binary = self.installed_runner_digest()?;
        let child = self.spawn_runner(&paths, jit)?;
        Ok(RunningJob { child, binary })
    }

    fn spawn_runner(
        &self,
        paths: &JobPaths,
        jit: &EncodedJit,
    ) -> Result<tokio::process::Child, HostError> {
        let (reader, mut writer) =
            std::io::pipe().map_err(|_| failure(RustRunnerFailure::JitDelivery))?;
        let bytes = Zeroizing::new(jit.expose().as_bytes().to_vec());
        std::thread::Builder::new()
            .spawn(move || {
                let _ = writer.write_all(&bytes);
            })
            .map_err(|_| failure(RustRunnerFailure::JitDelivery))?;
        let mut command = self.guest();
        command
            .args(PID_NAMESPACE)
            .arg(GUEST_RUNNER)
            .args(["run", "--jit", "@/dev/stdin"])
            .args(["--work-dir", paths.guest_work_dir()])
            .stdin(Stdio::from(reader))
            .process_group(0);
        tokio::process::Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| failure(RustRunnerFailure::Spawn))
    }
}

impl RunningJob {
    #[must_use]
    pub fn binary(&self) -> &str {
        &self.binary
    }

    pub async fn wait(&mut self) -> Result<ExitStatus, HostError> {
        let status = self.child.wait().await;
        status.map_err(|_| failure(RustRunnerFailure::Wait))
    }

    pub async fn stop(mut self) -> Result<(), HostError> {
        if let Some(pid) = self.child.id().and_then(|id| i32::try_from(id).ok())
            && let Some(group) = Pid::from_raw(pid)
        {
            let _ = kill_process_group(group, Signal::KILL);
        }
        let reaped = self.child.wait().await;
        reaped
            .map(drop)
            .map_err(|_| failure(RustRunnerFailure::Wait))
    }
}
