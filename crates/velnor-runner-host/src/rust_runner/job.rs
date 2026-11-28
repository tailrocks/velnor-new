use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{ExitStatus, Stdio};

use rustix::process::{Pid, Signal, kill_process_group};
use velnor_runner_github::EncodedJit;
use zeroize::Zeroizing;

use super::frames;
use super::guest_user::GuestUser;
use super::machine::failure;
use super::orb;
use super::store::Store;
use super::{GUEST_RUNNER, JobPaths, ReservationId, RustRunnerLauncher, TrustedFacts};
use crate::error::{HostError, RustRunnerFailure};

#[cfg(test)]
mod tests;

const BREACH_EXIT: i32 = 125;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JobBreach {
    Disk,
    ForeignDevice,
    MachineFull,
    Processes,
    Memory,
    Killed,
}

#[derive(serde::Deserialize)]
struct BreachRecord {
    breach: JobBreach,
}

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: tokio::process::Child,
    binary: String,
    launcher: RustRunnerLauncher,
    reservation: ReservationId,
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
        let frames = frames::encode(jit.expose(), &facts.to_file())
            .ok_or_else(|| failure(RustRunnerFailure::FrameDelivery))?;
        let child = self.spawn_runner(reservation, &paths, user, frames)?;
        Ok(RunningJob {
            child,
            binary,
            launcher: self.clone(),
            reservation,
        })
    }

    fn spawn_runner(
        &self,
        reservation: ReservationId,
        paths: &JobPaths,
        user: &GuestUser,
        frames: Zeroizing<Vec<u8>>,
    ) -> Result<tokio::process::Child, HostError> {
        let (reader, mut writer) =
            std::io::pipe().map_err(|_| failure(RustRunnerFailure::FrameDelivery))?;
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
            .args(["--max-jobs", &self.max_jobs.to_string()])
            .arg("--stdin-frames")
            .args(["--", GUEST_RUNNER, "run", "--jit", "-"])
            .args(["--work-dir", JobPaths::job_work_dir()])
            .stdin(Stdio::from(reader))
            .process_group(0);
        let child = tokio::process::Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| failure(RustRunnerFailure::Spawn))?;
        std::thread::Builder::new()
            .name("frame-writer".to_owned())
            .spawn(move || {
                let _ = writer.write_all(&frames);
            })
            .map_err(|_| failure(RustRunnerFailure::FrameDelivery))?;
        Ok(child)
    }
}

impl RunningJob {
    pub(super) fn binary(&self) -> &str {
        &self.binary
    }

    pub async fn wait(&mut self) -> Result<ExitStatus, HostError> {
        self.child
            .wait()
            .await
            .map_err(|_| failure(RustRunnerFailure::Wait))
    }

    pub(crate) async fn breach(&self, status: ExitStatus) -> Option<JobBreach> {
        if status.code() != Some(BREACH_EXIT) {
            return None;
        }
        let launcher = self.launcher.clone();
        let reservation = self.reservation;
        tokio::task::spawn_blocking(move || launcher.read_breach(reservation))
            .await
            .ok()
            .flatten()
    }

    pub async fn stop(mut self) -> Result<(), HostError> {
        if let Some(pid) = self.child.id().and_then(|id| i32::try_from(id).ok())
            && let Some(group) = Pid::from_raw(pid)
        {
            let _ = kill_process_group(group, Signal::KILL);
        }
        self.child
            .wait()
            .await
            .map(drop)
            .map_err(|_| failure(RustRunnerFailure::Wait))
    }
}

impl RustRunnerLauncher {
    fn read_breach(&self, reservation: ReservationId) -> Option<JobBreach> {
        let mut command = self.as_root();
        command
            .arg("/usr/bin/cat")
            .arg(JobPaths::of(reservation).guest_breach_file());
        let finished = orb::run(command, self.deadline, true).ok()?;
        if !finished.status.success() {
            return None;
        }
        serde_json::from_slice::<BreachRecord>(&finished.stdout)
            .ok()
            .map(|record| record.breach)
    }
}
