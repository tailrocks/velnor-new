// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::HostError;

mod controller;
mod frames;
mod guest_user;
mod job;
mod machine;
mod orb;
mod paths;
mod processes;
mod store;
mod sweep;
mod trusted;

pub use controller::{ConnectionPats, RustRunnerExit, run_rust_runner_host};
pub use guest_user::GuestUser;
pub(crate) use job::JobBreach;
pub use job::RunningJob;
pub use paths::ReservationId;
pub use processes::MachineProcess;
pub use store::HostHold;
pub use trusted::TrustedFacts;

pub(crate) use trusted::{PullRequestHead, TrustedEvent, Verified};

pub(crate) use controller::{Handover, heartbeat_readiness, run_blocking};
pub(crate) use paths::JobPaths;

const MACHINE_NAME: &str = "velnor";
const GUEST_JOBS: &str = "/run/velnor/jobs";
const GUEST_STORE: &str = "/var/lib/velnor";
const GUEST_RUNNER: &str = "/var/lib/velnor/bin/velnor-runner";
const DEFAULT_JOB_LIMIT_GIB: u64 = 64;
const DEFAULT_ORB: &str = "orb";
const ORB_DEADLINE: Duration = Duration::from_secs(60);
const ORB_CREATE_DEADLINE: Duration = Duration::from_mins(15);
const DEFAULT_HOST_STORE_UNDER_ROOT: &str = "Users/Shared/velnor";
const ORB_ENVIRONMENT: &str = "VELNOR_ORB";
const PRESERVED_ENVIRONMENT: [&str; 2] = ["PATH", "HOME"];
const HOST_DIRECTORIES: [&str; 7] = ["bin", "crates", "tools", "git", "gen", "cache", "records"];

static INSTALLED: OnceLock<RustRunnerLauncher> = OnceLock::new();

pub fn install_rust_runner_launcher(launcher: RustRunnerLauncher) -> Result<(), HostError> {
    INSTALLED.set(launcher).map_err(|_| HostError::Config)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustRunnerLauncher {
    orb: PathBuf,
    host_store: PathBuf,
    job_limit_gib: u64,
    max_jobs: u32,
    deadline: Duration,
}

impl RustRunnerLauncher {
    #[must_use]
    pub const fn new(orb: PathBuf, host_store: PathBuf) -> Self {
        Self {
            orb,
            host_store,
            job_limit_gib: DEFAULT_JOB_LIMIT_GIB,
            max_jobs: 1,
            deadline: ORB_DEADLINE,
        }
    }

    #[must_use]
    pub const fn with_job_limit_gib(mut self, job_limit_gib: u64) -> Self {
        self.job_limit_gib = job_limit_gib;
        self
    }

    #[must_use]
    pub const fn with_max_jobs(mut self, max_jobs: u32) -> Self {
        self.max_jobs = max_jobs;
        self
    }

    #[must_use]
    pub const fn with_orb_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    #[must_use]
    pub fn from_environment() -> Self {
        INSTALLED.get().cloned().unwrap_or_else(|| {
            let orb =
                std::env::var_os(ORB_ENVIRONMENT).map_or_else(|| DEFAULT_ORB.into(), Into::into);
            Self::new(orb, Path::new("/").join(DEFAULT_HOST_STORE_UNDER_ROOT))
        })
    }

    #[must_use]
    pub fn host_store(&self) -> &Path {
        &self.host_store
    }

    fn orb(&self) -> Command {
        let mut command = Command::new(&self.orb);
        command.env_clear();
        for name in PRESERVED_ENVIRONMENT {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
    }

    fn in_machine(&self) -> Command {
        let mut command = self.orb();
        command.args(["-m", MACHINE_NAME]);
        command
    }

    fn as_root(&self) -> Command {
        let mut command = self.in_machine();
        command.args(["-u", "root"]);
        command
    }
}
