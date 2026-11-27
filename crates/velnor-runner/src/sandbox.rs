// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod apply;
mod args;
mod cgroup;
mod dirs;
mod disk;
mod layout;
#[cfg(target_os = "linux")]
mod mounts;
mod parent;
mod plan;
#[cfg(target_os = "linux")]
mod seal;
mod trusted;

pub use apply::apply;
pub use args::{ApplyArgs, SandboxArgs};
pub use layout::{Layout, RootLink};
pub use parent::run;
pub use plan::{MountOp, plan};

use clap::ValueEnum;

pub const STORE_ROOT: &str = "/var/lib/velnor";
pub const GEN_ROOT: &str = "/srv/velnor/gen";
pub const CGROUP_ROOT: &str = "/sys/fs/cgroup/velnor";
pub const SANDBOX_ROOTS: &str = "/run/velnor-sandbox";
pub const DEFAULT_DISK_LIMIT_GIB: u64 = 64;
pub const RESOLV_CONF: &str = "/opt/orbstack-guest/etc/resolv.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Profile {
    Job,
}

#[cfg(test)]
mod tests;
