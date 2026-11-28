// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod apply;
mod args;
mod cgroup;
mod cleanup;
mod dirs;
mod disk;
mod flatten;
mod floor;
mod frames;
mod jit;
mod layout;
#[cfg(target_os = "linux")]
mod mounts;
mod net;
mod parent;
mod plan;
mod run_dir;
#[cfg(target_os = "linux")]
mod seal;
#[cfg(target_os = "linux")]
mod seccomp;
mod tree;
mod trusted;
mod watch;

pub use apply::apply;
pub use args::{ApplyArgs, CleanupArgs, SandboxArgs};
pub use cleanup::run as cleanup;
pub use layout::{Layout, RootLink};
pub use parent::run;
pub use plan::{MountOp, plan};

use clap::ValueEnum;

pub const STORE_ROOT: &str = "/var/lib/velnor";
pub const GEN_ROOT: &str = "/srv/velnor/gen";
pub const CGROUP_ROOT: &str = "/sys/fs/cgroup/velnor";
pub const SANDBOX_ROOTS: &str = "/run/velnor-sandbox";
pub const JOB_WORK: &str = "/work";
pub const JOBS_ROOT: &str = "/run/velnor/jobs";
pub const WEDGED_EXIT: u8 = 70;
pub const DEFAULT_DISK_LIMIT_GIB: u64 = 64;
pub const DEFAULT_MACHINE_FLOOR_GIB: u64 = 4;
pub const DEFAULT_MAX_JOBS: u32 = 1;
pub const PIDS_MAX: u64 = 16384;
pub const MEMORY_PERCENT: u64 = 90;
pub const NET_LOCK: &str = "/var/lib/velnor/locks/net.lock";
pub const RESOLV_CONF: &str = "/opt/orbstack-guest/etc/resolv.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Profile {
    Job,
}

#[cfg(test)]
mod tests;
