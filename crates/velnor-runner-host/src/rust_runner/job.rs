use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};

use rustix::process::{Pid, Signal, kill_process_group};
use velnor_runner_github::EncodedJit;

use super::guest_user::GuestUser;
use super::machine::failure;
use super::store::Store;
use super::{GUEST_RUNNER, JobPaths, ReservationId, RustRunnerLauncher, TrustedFacts};
use crate::error::{HostError, RustRunnerFailure};

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: tokio::process::Child,
    store: PathBuf,
    trusted_name: String,
    binary: String,
}

impl RustRunnerLauncher {
    pub fn start_job(
        &self,
        reservation: ReservationId,
        jit: &EncodedJit,
        facts: &TrustedFacts,
        user: &GuestUser,
    ) -> Result<RunningJob, HostError> {
        let paths = JobPaths::of(reservation);
        let store = Store::open(&self.host_store)?;
        store.require_runner_binary()?;
        let binary = store.runner_digest()?;
        store.write_trusted(paths.trusted_name(), &facts.to_file())?;
        match self.spawn_runner(reservation, &paths, user, jit) {
            Ok(child) => Ok(RunningJob {
                child,
                store: self.host_store.clone(),
                trusted_name: paths.trusted_name().to_owned(),
                binary,
            }),
            Err(error) => {
                let _ = store.remove_trusted(paths.trusted_name());
                Err(error)
            }
        }
    }

    fn spawn_runner(
        &self,
        reservation: ReservationId,
        paths: &JobPaths,
        user: &GuestUser,
        jit: &EncodedJit,
    ) -> Result<tokio::process::Child, HostError> {
        let (reader, mut writer) =
            std::io::pipe().map_err(|_| failure(RustRunnerFailure::JitDelivery))?;
        let mut command = self.as_root();
        command
            .arg(GUEST_RUNNER)
            .args(["sandbox", "--profile", "job", "--reservation"])
            .arg(reservation.to_string())
            .args(["--uid", &user.uid.to_string()])
            .args(["--gid", &user.gid.to_string()])
            .args(["--home", &user.home])
            .args(["--work-dir", paths.guest_work_dir()])
            .args(["--disk-limit-gib", &self.job_limit_gib.to_string()])
            .arg("--jit-stdin")
            .args(["--trusted", paths.guest_trusted_file()])
            .args(["--", GUEST_RUNNER, "run", "--jit", "-"])
            .args(["--work-dir", paths.guest_work_dir()])
            .stdin(Stdio::from(reader))
            .process_group(0);
        let child = tokio::process::Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| failure(RustRunnerFailure::Spawn))?;
        writer
            .write_all(jit.expose().as_bytes())
            .map_err(|_| failure(RustRunnerFailure::JitDelivery))?;
        Ok(child)
    }
}

impl RunningJob {
    pub(super) fn binary(&self) -> &str {
        &self.binary
    }

    pub async fn wait(&mut self) -> Result<ExitStatus, HostError> {
        let status = self.child.wait().await;
        self.remove_trusted_file();
        status.map_err(|_| failure(RustRunnerFailure::Wait))
    }

    pub async fn stop(mut self) -> Result<(), HostError> {
        if let Some(pid) = self.child.id().and_then(|id| i32::try_from(id).ok())
            && let Some(group) = Pid::from_raw(pid)
        {
            let _ = kill_process_group(group, Signal::KILL);
        }
        let reaped = self.child.wait().await;
        self.remove_trusted_file();
        reaped
            .map(drop)
            .map_err(|_| failure(RustRunnerFailure::Wait))
    }

    fn remove_trusted_file(&self) {
        let _ = Store::open(&self.store).and_then(|store| store.remove_trusted(&self.trusted_name));
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        self.remove_trusted_file();
    }
}
