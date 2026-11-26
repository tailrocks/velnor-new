// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

mod job;
mod machine;

pub use job::RunningJob;

const MACHINE_NAME: &str = "velnor";
const GUEST_STORE: &str = "/var/lib/velnor";
const GUEST_RUNNER: &str = "/var/lib/velnor/bin/velnor-runner";
const DEFAULT_ORB: &str = "orb";
const DEFAULT_HOST_STORE_UNDER_ROOT: &str = "Users/Shared/velnor";
const ORB_ENVIRONMENT: &str = "VELNOR_ORB";
const PID_NAMESPACE: [&str; 6] = [
    "unshare",
    "--user",
    "--map-current-user",
    "--pid",
    "--fork",
    "--kill-child",
];
const HOST_DIRECTORIES: [&str; 8] = [
    "bin", "crates", "tools", "git", "gen", "cache", "records", "jit",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustRunnerLauncher {
    orb: PathBuf,
    host_store: PathBuf,
}

impl RustRunnerLauncher {
    #[must_use]
    pub const fn new(orb: PathBuf, host_store: PathBuf) -> Self {
        Self { orb, host_store }
    }

    #[must_use]
    pub fn from_environment() -> Self {
        let orb = std::env::var_os(ORB_ENVIRONMENT).map_or_else(|| DEFAULT_ORB.into(), Into::into);
        Self::new(orb, Path::new("/").join(DEFAULT_HOST_STORE_UNDER_ROOT))
    }

    #[must_use]
    pub fn host_store(&self) -> &Path {
        &self.host_store
    }

    fn orb(&self) -> Command {
        Command::new(&self.orb)
    }

    fn in_machine(&self) -> Command {
        let mut command = self.orb();
        command.args(["-m", MACHINE_NAME]);
        command
    }
}
