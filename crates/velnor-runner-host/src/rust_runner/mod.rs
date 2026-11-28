// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::HostError;

mod atomic;
mod census;
mod controller;
mod doctor;
mod job;
mod machine;
mod orb;
mod paths;
mod processes;
mod store;
mod sweep;
mod teardown;

pub use census::{
    parse_marker_token, wait_for_rust_runner_connection_admitted,
    wait_for_rust_runner_connection_drain, wait_for_rust_runner_drain,
};
pub use controller::{
    ConnectionPats, Handover, PatSource, RustRunnerExit, ShutdownSignal, load_connection_pat,
    run_rust_runner_host, supervise_rust_runner_host,
};
pub use doctor::run_rust_runner_doctor_probe;
pub use job::RunningJob;
pub use paths::ReservationId;
pub use processes::MachineProcess;
pub use store::HostHold;
pub use teardown::detach_rust_runner_connection;

use store::Store;

pub use controller::host_toml_sha256;
pub(crate) use controller::{heartbeat_readiness, run_blocking};
pub(crate) use paths::JobPaths;

const MACHINE_NAME: &str = "velnor";
const GUEST_STORE: &str = "/var/lib/velnor";
const GUEST_RUNNER: &str = "/var/lib/velnor/bin/velnor-runner";
const DEFAULT_ORB: &str = "orb";
const ORB_DEADLINE: Duration = Duration::from_secs(60);
const ORB_CREATE_DEADLINE: Duration = Duration::from_mins(15);
const DEFAULT_LISTER: &str = "/bin/ls";
const DEFAULT_HOST_STORE_UNDER_ROOT: &str = "Users/Shared/velnor";
const ORB_ENVIRONMENT: &str = "VELNOR_ORB";
const PRESERVED_ENVIRONMENT: [&str; 2] = ["PATH", "HOME"];
const PID_NAMESPACE: [&str; 6] = [
    "unshare",
    "--user",
    "--map-current-user",
    "--pid",
    "--fork",
    "--kill-child",
];
const HOST_DIRECTORIES: [&str; 8] = [
    "bin", "crates", "tools", "git", "gen", "cache", "records", "work",
];
const JIT_DIRECTORY: &str = "jit";
const WORK_DIRECTORY: &str = "work";

static INSTALLED: OnceLock<RustRunnerLauncher> = OnceLock::new();

pub fn install_rust_runner_launcher(launcher: RustRunnerLauncher) -> Result<(), HostError> {
    INSTALLED.set(launcher).map_err(|_| HostError::Config)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustRunnerLauncher {
    orb: PathBuf,
    host_store: PathBuf,
    deadline: Duration,
    lister: PathBuf,
}

impl RustRunnerLauncher {
    #[must_use]
    pub fn new(orb: PathBuf, host_store: PathBuf) -> Self {
        Self {
            orb,
            host_store,
            deadline: ORB_DEADLINE,
            lister: DEFAULT_LISTER.into(),
        }
    }

    #[must_use]
    pub const fn with_orb_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    #[must_use]
    pub fn with_lister(mut self, lister: PathBuf) -> Self {
        self.lister = lister;
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

    fn store(&self) -> Result<Store, HostError> {
        Store::open(&self.host_store).map(|store| store.listed_by(&self.lister))
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
