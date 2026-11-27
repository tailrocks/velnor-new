// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::HostError;

mod controller;
mod job;
mod machine;
mod orb;
mod paths;
mod processes;
mod provision;
mod runner_binary;
mod shape;
mod store;
mod sweep;

pub use controller::{ConnectionPats, RustRunnerExit, run_rust_runner_host};
pub use job::RunningJob;
pub use paths::ReservationId;
pub use processes::MachineProcess;
pub use shape::GuestUser;
pub use store::HostHold;

pub(crate) use controller::{Handover, heartbeat_readiness, run_blocking};
pub(crate) use paths::{GUEST_GEN, JobPaths};

const MACHINE_NAME: &str = "velnor";
const GUEST_SHARE: &str = "/var/lib/velnor-host";
const GUEST_STORE: &str = "/var/lib/velnor";
const GUEST_RUNNER: &str = "/var/lib/velnor/bin/velnor-runner";
const GUEST_SRV: &str = "/srv/velnor";
const GUEST_CAP_RECORD: &str = "/var/lib/velnor/cap-probe";
const GIB: u64 = 1 << 30;
const DEFAULT_ORB: &str = "orb";
const ORB_DEADLINE: Duration = Duration::from_secs(60);
const ORB_CREATE_DEADLINE: Duration = Duration::from_mins(15);
const DEFAULT_HOST_STORE_UNDER_ROOT: &str = "Users/Shared/velnor";
const ORB_ENVIRONMENT: &str = "VELNOR_ORB";
const RUNNER_BINARY_ENVIRONMENT: &str = "VELNOR_RUNNER_BINARY";
const RUNNER_BINARY_NAME: &str = "velnor-runner";
const PRESERVED_ENVIRONMENT: [&str; 2] = ["PATH", "HOME"];
const GUEST_ENVIRONMENT: [&str; 4] = [
    "/usr/bin/env",
    "-i",
    "PATH=/usr/sbin:/usr/bin:/sbin:/bin",
    "LC_ALL=C",
];
const PID_NAMESPACE: [&str; 6] = [
    "unshare",
    "--user",
    "--map-current-user",
    "--pid",
    "--fork",
    "--kill-child",
];
const PRIVATE_DIRECTORIES: [&str; 2] = [JIT_DIRECTORY, TRUSTED_DIRECTORY];
const JIT_DIRECTORY: &str = "jit";
const TRUSTED_DIRECTORY: &str = "trusted";

fn default_runner_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(RUNNER_BINARY_NAME)))
        .unwrap_or_else(|| PathBuf::from(RUNNER_BINARY_NAME))
}

static INSTALLED: OnceLock<RustRunnerLauncher> = OnceLock::new();

pub fn install_rust_runner_launcher(launcher: RustRunnerLauncher) -> Result<(), HostError> {
    INSTALLED.set(launcher).map_err(|_| HostError::Config)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustRunnerLauncher {
    orb: PathBuf,
    host_store: PathBuf,
    runner_binary: PathBuf,
    disk_cap_gib: u64,
    job_limit_gib: u64,
    deadline: Duration,
}

impl RustRunnerLauncher {
    #[must_use]
    pub const fn new(orb: PathBuf, host_store: PathBuf, runner_binary: PathBuf) -> Self {
        Self {
            orb,
            host_store,
            runner_binary,
            disk_cap_gib: crate::config::DEFAULT_ONE_JOB_CAP_GIB,
            job_limit_gib: crate::config::DEFAULT_JOB_LIMIT_GIB,
            deadline: ORB_DEADLINE,
        }
    }

    #[must_use]
    pub const fn with_machine_disk(mut self, disk_cap_gib: u64, job_limit_gib: u64) -> Self {
        self.disk_cap_gib = disk_cap_gib;
        self.job_limit_gib = job_limit_gib;
        self
    }

    #[must_use]
    pub const fn job_limit_gib(&self) -> u64 {
        self.job_limit_gib
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
            let runner_binary = std::env::var_os(RUNNER_BINARY_ENVIRONMENT)
                .map_or_else(default_runner_binary, Into::into);
            Self::new(
                orb,
                Path::new("/").join(DEFAULT_HOST_STORE_UNDER_ROOT),
                runner_binary,
            )
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

    fn guest(&self) -> Command {
        let mut command = self.orb();
        command
            .args(["-m", MACHINE_NAME, "-u", "root"])
            .args(GUEST_ENVIRONMENT);
        command
    }
}
