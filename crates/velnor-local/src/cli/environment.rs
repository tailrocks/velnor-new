// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, ValueEnum};

use super::StorageArgs;
use crate::pairs::{Limits, parse_cpus, parse_memory};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RunnerMode {
    Pair,
    Plain,
}

#[derive(Clone, Debug, Args)]
pub struct EnvironmentArgs {
    #[arg(
        long,
        help = "State directory of this environment: keys, cache, API stub state, events and logs"
    )]
    pub dir: PathBuf,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..), help = "Number of runner containers, or of pairs in pair mode, at least one")]
    pub runners: u16,
    #[arg(
        long,
        default_value_t = 0,
        help = "Number of runner containers, or of pairs in pair mode, for untrusted runs, on a network without the API stub but with whatever else Docker containers reach (the host, the LAN, the internet); run starts one when a scenario opens a pull request from a fork"
    )]
    pub fork_runners: u16,
    #[arg(
        long,
        default_value = "main",
        help = "Default branch of the repository"
    )]
    pub default_branch: String,
    #[arg(
        long,
        value_enum,
        default_value_t = RunnerMode::Pair,
        help = "Job containers: pair runs each job in a velnor-host dind and runner pair behind the job-start check; plain runs the official runner image without either"
    )]
    pub runner: RunnerMode,
    #[arg(
        long,
        help = "Approved job-authority policy file (the JSON that velnor-host connect --job-authority-policy reads); turns the approved-workflows check on in pair mode: a job it does not approve is refused at job start with reason policy"
    )]
    pub policy: Option<PathBuf>,
    #[arg(
        long,
        help = "velnor-host built with local mode, which pair mode runs; `velnor-local build` builds it. Default: the one built in the checkout that holds this executable, never one from the working directory"
    )]
    pub velnor_host: Option<PathBuf>,
    #[arg(
        long,
        help = "linux-arm64 velnor-ci that setup-velnor installs instead of the pinned release, with a warning that names its SHA-256. Read-only in the trusted runners only: mounted into the plain runners, or with --runner pair copied by velnor-host into the trusted pairs' runner volume and mounted at the same path. The workflow stays unmodified and still pins the release"
    )]
    pub velnor_ci_binary: Option<PathBuf>,
    #[arg(
        long,
        value_delimiter = ',',
        value_parser = runner_label,
        help = "Runner label to register besides the built-in ubuntu-* ones, so a workflow whose runs-on names another label (for example ubuntu-26.04-scale-set) runs unchanged; repeatable or comma separated; letters, digits, '.', '_' and '-' only"
    )]
    pub label: Vec<String>,
    #[arg(
        long,
        help = "Start the API stub with --job-cache-admin: every job can list and delete the cache entries of every scope through the GitHub cache API, whatever its permissions and event. Off by default: a job then needs `actions: read` to list and `actions: write` to delete, on an event that is not a pull request, as on GitHub, and gets 403 otherwise"
    )]
    pub job_cache_admin: bool,
    #[arg(
        long,
        default_value = "6g",
        value_name = "SIZE",
        value_parser = parse_memory,
        help = "Memory limit of each runner in pair mode (the dind keeps 2 GiB); a whole number with m or g, at least 512m. Plain mode sets no limits"
    )]
    pub runner_memory: u64,
    #[arg(
        long,
        default_value = "4",
        value_name = "N",
        value_parser = parse_cpus,
        help = "CPU limit of each runner in pair mode, decimals allowed (1.5); stored in millicores. Plain mode sets no limits"
    )]
    pub runner_cpus: u64,
    #[command(flatten)]
    pub storage: StorageArgs,
}

pub fn runner_label(text: &str) -> Result<String, String> {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Ok(text.to_owned());
    }
    Err(format!(
        "runner label {text:?} must be non-empty and hold only letters, digits, '.', '_' and '-'"
    ))
}

impl Default for EnvironmentArgs {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            runners: 2,
            fork_runners: 0,
            default_branch: "main".to_owned(),
            runner: RunnerMode::Pair,
            policy: None,
            velnor_host: None,
            velnor_ci_binary: None,
            label: Vec::new(),
            job_cache_admin: false,
            runner_memory: Limits::default().runner_memory_bytes,
            runner_cpus: Limits::default().runner_cpu_millicores,
            storage: StorageArgs::default(),
        }
    }
}

impl EnvironmentArgs {
    #[must_use]
    pub const fn limits(&self) -> Limits {
        Limits {
            runner_cpu_millicores: self.runner_cpus,
            runner_memory_bytes: self.runner_memory,
        }
    }

    #[must_use]
    pub const fn containers(&self, runners: u16) -> u16 {
        match self.runner {
            RunnerMode::Plain => runners,
            RunnerMode::Pair => 0,
        }
    }
}
