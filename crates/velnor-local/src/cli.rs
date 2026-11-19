// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::pairs::{
    DEFAULT_RUNNER_CPU_MILLICORES, DEFAULT_RUNNER_MEMORY_BYTES, Limits, parse_memory,
};
use crate::side::Side;
use crate::timeout::{TimeoutScale, parse_timeout_scale};

mod environment;
mod servers;
mod sizes;

pub use environment::{EnvironmentArgs, RunnerMode, runner_label};
pub use servers::{ApiStubArgs, CacheServerArgs, ControlArgs};
pub use sizes::{StorageArgs, SuperviseSizes};

#[derive(Debug, Parser)]
#[command(name = "velnor-local", version, about = "The local GitHub environment")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(
        about = "Serve GitHub's cache protocol from local disk under GitHub's cache rules, and forward every other request to runner.server"
    )]
    CacheServer(CacheServerArgs),
    #[command(
        about = "Answer the GitHub API calls velnor-ci makes from a local git repository, a file of open pull requests and the cache server's index"
    )]
    ApiStub(ApiStubArgs),
    #[command(
        about = "Start runner.server, the cache server, the API stub and linux/arm64 runner containers for one state directory"
    )]
    Up(UpArgs),
    #[command(about = "Stop and remove the containers and the network of one state directory")]
    Down(DownArgs),
    #[command(
        about = "Start an environment, replay a scenario against a copy of a local repository, stop the environment and print a verdict"
    )]
    Run(RunArgs),
    #[command(
        about = "Compare the probe observations in the logs of a run with the probe results stored from GitHub"
    )]
    Conformance(ConformanceArgs),
    #[command(
        about = "Build the linux/arm64 runner, dind and probe images that pair mode runs, tagged velnor-local/pair-*, and velnor-host with local mode"
    )]
    Build(BuildArgs),
    #[command(
        about = "Check Docker, the images and velnor-host with local mode; print ok or miss per part with the fix, and exit 1 on any miss"
    )]
    Doctor(DoctorArgs),
    #[command(
        about = "Show the mode, the authority, the ledger size, each pair and the last refusal of one state directory"
    )]
    Status(DownArgs),
    #[command(
        hide = true,
        about = "Register a throwaway ephemeral runner at runner.server and write its configuration as a JIT file"
    )]
    Jit(JitArgs),
    #[command(
        hide = true,
        about = "Serve the local authority on 127.0.0.1: the ledger of dispatched events, run facts, JITs and agent deletion; `up` starts it"
    )]
    Supervise(SuperviseArgs),
}

#[derive(Debug, Args)]
pub struct SuperviseArgs {
    #[arg(long, help = "State directory of the environment")]
    pub dir: PathBuf,
    #[arg(
        long,
        help = "velnor-host built with local mode; when given, supervise runs it with the pairs below"
    )]
    pub velnor_host: Option<PathBuf>,
    #[arg(long, default_value_t = 0, help = "Trusted pairs")]
    pub pairs: u16,
    #[arg(long, default_value_t = 0, help = "Untrusted pairs")]
    pub fork_pairs: u16,
    #[arg(
        long,
        help = "Apply runner/policy.json, which `up` copied from --policy"
    )]
    pub policy: bool,
    #[arg(long, help = "Repository whose workflow files the policy hashes")]
    pub repo: Option<PathBuf>,
    #[arg(
        long,
        value_parser = runner_label,
        help = "Extra runner label to register for every minted runner; `up` passes the --label values"
    )]
    pub label: Vec<String>,
    #[arg(long, default_value_t = DEFAULT_RUNNER_MEMORY_BYTES, help = "Memory limit of each runner container, in bytes")]
    pub runner_memory_bytes: u64,
    #[arg(long, default_value_t = DEFAULT_RUNNER_CPU_MILLICORES, help = "CPU limit of each runner container, in millicores")]
    pub runner_cpu_millicores: u64,
    #[command(flatten)]
    pub sizes: SuperviseSizes,
    #[arg(
        long,
        help = "Absolute path of the linux-arm64 velnor-ci that velnor-host puts into each trusted pair's runner at /opt/velnor/local; `up --velnor-ci-binary` checked it"
    )]
    pub velnor_ci_binary: Option<PathBuf>,
}

impl SuperviseArgs {
    #[must_use]
    pub const fn limits(&self) -> Limits {
        Limits {
            runner_cpu_millicores: self.runner_cpu_millicores,
            runner_memory_bytes: self.runner_memory_bytes,
        }
    }
}

#[derive(Debug, Args)]
pub struct BuildArgs {
    #[arg(
        long,
        default_value = ".",
        help = "Repository root that holds images/ and .velnor/runner.lock"
    )]
    pub root: PathBuf,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(
        long,
        default_value = "6g",
        value_name = "SIZE",
        value_parser = parse_memory,
        help = "Runner memory that the run will ask for (as `up --runner-memory`), so the Docker VM memory check matches it"
    )]
    pub runner_memory: u64,
    #[arg(
        long,
        help = "velnor-host to check instead of the one built in the checkout that holds this executable"
    )]
    pub velnor_host: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct JitArgs {
    #[arg(long, help = "State directory of the environment")]
    pub dir: PathBuf,
    #[arg(
        long,
        value_enum,
        help = "Side whose runner.server registers the runner"
    )]
    pub side: Side,
}

#[derive(Debug, Args)]
pub struct ConformanceArgs {
    #[arg(
        long,
        help = "State directory of the run whose logs hold the observations"
    )]
    pub dir: PathBuf,
    #[arg(
        long,
        default_value = "local/conformance",
        help = "Directory of the probe results stored from GitHub"
    )]
    pub conformance: PathBuf,
}

#[derive(Debug, Args)]
pub struct UpArgs {
    #[command(flatten)]
    pub environment: EnvironmentArgs,
    #[arg(long, help = "Local git repository that the API stub reads")]
    pub repo: PathBuf,
}

#[derive(Debug, Args)]
pub struct DownArgs {
    #[arg(long, help = "State directory of the environment")]
    pub dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(
        required = true,
        help = "Scenario names in --scenarios, or paths of scenario files; with several, each runs in <dir>/<name>"
    )]
    pub scenario: Vec<String>,
    #[arg(
        long,
        default_value_t = 1,
        value_parser = clap::value_parser!(u16).range(1..),
        help = "How many scenarios run at the same time, each in its own environment"
    )]
    pub parallel: u16,
    #[command(flatten)]
    pub environment: EnvironmentArgs,
    #[arg(
        long,
        help = "Local git checkout; it is copied with its uncommitted changes and never modified"
    )]
    pub repo: PathBuf,
    #[arg(
        long,
        default_value = "local/scenarios",
        help = "Directory of scenario files"
    )]
    pub scenarios: PathBuf,
    #[arg(
        long,
        help = "Workflow file relative to the repository; every workflow runs when omitted"
    )]
    pub workflow: Option<String>,
    #[arg(
        long,
        default_value = "local/repository",
        help = "Repository as owner/name, as github.repository reports it"
    )]
    pub repository: String,
    #[arg(
        long,
        value_parser = clap::value_parser!(u64).range(1..),
        help = "GitHub's numeric id of the repository, which the --policy file names; events carry it instead of the id derived from --repository"
    )]
    pub repository_id: Option<u64>,
    #[arg(
        long,
        default_value = "velnor-ci",
        help = "velnor-ci program that writes the metrics file of each run"
    )]
    pub velnor_ci: PathBuf,
    #[arg(
        long,
        help = "Leave the environment up after the verdict, for status and curl; `down` removes it"
    )]
    pub keep: bool,
    #[arg(
        long,
        help = "Start from an empty cache instead of the one that an earlier run on the same --dir left"
    )]
    pub fresh_cache: bool,
    #[arg(
        long,
        default_value = "1",
        value_name = "FACTOR",
        value_parser = parse_timeout_scale,
        help = "Multiply every numeric timeout-minutes in the run's copy of the workflows (job and step level) by FACTOR, rounding each up to whole minutes, for a slower local machine; at least 1, at most three decimals (3 on a laptop). Expressions are left alone, and the checkout is not modified"
    )]
    pub timeout_scale: TimeoutScale,
}
