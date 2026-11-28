// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, ensure};
use clap::Args;

use super::layout::work_dir_of;
use super::{
    DEFAULT_DISK_LIMIT_GIB, DEFAULT_MACHINE_FLOOR_GIB, DEFAULT_MAX_JOBS, Profile, STORE_ROOT,
};

const STORE_SCOPES: [&str; 2] = ["cache", "git"];

#[derive(Debug, Args)]
pub struct SandboxArgs {
    #[arg(long, value_enum, help = "What the sandbox is for")]
    pub profile: Profile,
    #[arg(
        long,
        value_name = "ID",
        help = "Reservation of the job; names its cgroup and its directories"
    )]
    pub reservation: u64,
    #[arg(long, value_name = "UID", help = "User the command runs as; never 0")]
    pub uid: u32,
    #[arg(long, value_name = "GID", help = "Group the command runs as; never 0")]
    pub gid: u32,
    #[arg(
        long,
        value_name = "DIR",
        help = "HOME of the command; a fresh directory of the job is mounted here"
    )]
    pub home: PathBuf,
    #[arg(
        long,
        value_name = "DIR",
        help = "Work directory of the job: <generations root>/work/<reservation>"
    )]
    pub work_dir: PathBuf,
    #[arg(
        long,
        help = "Read two length-prefixed frames (4 bytes big-endian, at most 64 KiB each) from standard input: the JIT configuration, which the command receives through a pipe that closes after the write, then the trusted facts, which land in a root-only tmpfs file"
    )]
    pub stdin_frames: bool,
    #[arg(
        long,
        value_name = "GIB",
        default_value_t = DEFAULT_DISK_LIMIT_GIB,
        value_parser = clap::value_parser!(u64).range(1..=4096),
        help = "Visible disk the job may use in its work dir, home, tmp and stage before it is killed; what the walk cannot see is bounded by the free-space floor"
    )]
    pub disk_limit_gib: u64,
    #[arg(
        long,
        value_name = "JOBS",
        default_value_t = DEFAULT_MAX_JOBS,
        value_parser = clap::value_parser!(u32).range(1..=1024),
        help = "Jobs the machine runs at once; each gets this share of 90% of the machine's memory"
    )]
    pub max_jobs: u32,
    #[arg(
        long,
        value_name = "GIB",
        default_value_t = DEFAULT_MACHINE_FLOOR_GIB,
        value_parser = clap::value_parser!(u64).range(0..=4096),
        help = "Kill the job when the free space of the generations filesystem drops below this"
    )]
    pub machine_floor_gib: u64,
    #[arg(
        long = "store-read-only",
        value_name = "DIR",
        help = "A directory under the cache or git store that the job may read; repeatable"
    )]
    pub store_read_only: Vec<PathBuf>,
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("scope").required(true))]
pub struct CleanupArgs {
    #[arg(
        long,
        value_name = "ID",
        group = "scope",
        help = "Reservation whose cgroup and directories are removed"
    )]
    pub reservation: Option<u64>,
    #[arg(
        long,
        group = "scope",
        help = "Remove the cgroup and the directories of every job"
    )]
    pub all: bool,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[arg(long, value_name = "JSON", help = "Layout the sandbox parent computed")]
    pub layout: String,
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

pub(super) fn normalized(path: &Path) -> bool {
    let mut parts = path.components();
    parts.next() == Some(Component::RootDir)
        && parts.all(|part| matches!(part, Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

fn is_store_scope(path: &Path) -> bool {
    normalized(path)
        && STORE_SCOPES.iter().any(|scope| {
            path.strip_prefix(Path::new(STORE_ROOT).join(scope))
                .is_ok_and(|rest| rest.components().next().is_some())
        })
}

impl SandboxArgs {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(self.reservation != 0, "the reservation must not be 0");
        ensure!(
            self.uid != 0 && self.gid != 0,
            "the job runs as a user other than root"
        );
        ensure!(
            normalized(&self.home)
                && self.home.starts_with("/home")
                && self.home.components().count() > 2,
            "--home {} is not a directory under /home",
            self.home.display()
        );
        ensure!(
            self.work_dir == work_dir_of(self.reservation),
            "--work-dir {} is not {}",
            self.work_dir.display(),
            work_dir_of(self.reservation).display()
        );
        for path in &self.store_read_only {
            ensure!(
                is_store_scope(path),
                "--store-read-only {} is not a directory under the cache or git store",
                path.display()
            );
        }
        ensure!(
            self.command
                .first()
                .is_some_and(|program| Path::new(program).is_absolute()),
            "the command must start with an absolute path"
        );
        Ok(())
    }
}
