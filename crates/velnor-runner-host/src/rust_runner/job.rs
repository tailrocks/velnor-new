use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::OnceLock;

use rustix::process::{Pid, Signal, kill_process_group};
use velnor_runner_github::EncodedJit;

use super::machine::failure;
use super::store::Store;
use super::{GUEST_RUNNER, JobPaths, ReservationId, RustRunnerLauncher, TrustedFacts, orb};
use crate::error::{HostError, RustRunnerFailure};

const GUEST_USER_PROBE: &str = r#"echo "$(id -u) $(id -g) $HOME""#;

static GUEST_USER: OnceLock<GuestUser> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct GuestUser {
    uid: u32,
    gid: u32,
    home: String,
}

fn parse_guest_user(output: &str) -> Option<GuestUser> {
    let mut fields = output.split_whitespace();
    let user = GuestUser {
        uid: fields.next()?.parse().ok()?,
        gid: fields.next()?.parse().ok()?,
        home: fields.next()?.to_owned(),
    };
    fields.next().is_none().then_some(user)
}

#[must_use]
#[derive(Debug)]
pub struct RunningJob {
    child: tokio::process::Child,
    store: PathBuf,
    jit_name: String,
    trusted_name: String,
    binary: String,
}

impl RustRunnerLauncher {
    pub fn start_job(
        &self,
        reservation: ReservationId,
        jit: &EncodedJit,
        facts: &TrustedFacts,
    ) -> Result<RunningJob, HostError> {
        let paths = JobPaths::of(reservation);
        let store = Store::open(&self.host_store)?;
        store.require_runner_binary()?;
        let binary = store.runner_digest()?;
        let user = self.guest_user()?;
        store.write_jit(paths.trusted_name(), &facts.to_file())?;
        let written = store.write_jit(paths.jit_name(), jit.expose().as_bytes());
        let spawned = written.and_then(|()| self.spawn_runner(reservation, &paths, &user));
        match spawned {
            Ok(child) => Ok(RunningJob {
                child,
                store: self.host_store.clone(),
                jit_name: paths.jit_name().to_owned(),
                trusted_name: paths.trusted_name().to_owned(),
                binary,
            }),
            Err(error) => {
                let _ = store.remove_jit(paths.jit_name());
                let _ = store.remove_jit(paths.trusted_name());
                Err(error)
            }
        }
    }

    fn guest_user(&self) -> Result<GuestUser, HostError> {
        if let Some(user) = GUEST_USER.get() {
            return Ok(user.clone());
        }
        let mut command = self.in_machine();
        command.args(["sh", "-c", GUEST_USER_PROBE]);
        let output = orb::run(command, self.deadline, true).map_err(failure)?;
        let text = String::from_utf8(output.stdout).ok();
        let user = text
            .as_deref()
            .filter(|_| output.status.success())
            .and_then(parse_guest_user)
            .ok_or_else(|| failure(RustRunnerFailure::GuestUser))?;
        Ok(GUEST_USER.get_or_init(|| user).clone())
    }

    fn spawn_runner(
        &self,
        reservation: ReservationId,
        paths: &JobPaths,
        user: &GuestUser,
    ) -> Result<tokio::process::Child, HostError> {
        let mut command = self.as_root();
        command
            .arg(GUEST_RUNNER)
            .args(["sandbox", "--profile", "job", "--reservation"])
            .arg(reservation.to_string())
            .args(["--uid", &user.uid.to_string()])
            .args(["--gid", &user.gid.to_string()])
            .args(["--home", &user.home])
            .args(["--work-dir", paths.guest_work_dir()])
            .args(["--jit", paths.guest_jit_file()])
            .args(["--trusted", paths.guest_trusted_file()])
            .args(["--", GUEST_RUNNER, "run", "--jit", "-"])
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
        if let Ok(store) = Store::open(&self.store) {
            let _ = store.remove_jit(&self.jit_name);
            let _ = store.remove_jit(&self.trusted_name);
        }
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        self.remove_jit_file();
    }
}
