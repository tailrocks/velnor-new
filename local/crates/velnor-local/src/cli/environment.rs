// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, ValueEnum};

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
        }
    }
}

impl EnvironmentArgs {
    #[must_use]
    pub const fn containers(&self, runners: u16) -> u16 {
        match self.runner {
            RunnerMode::Plain => runners,
            RunnerMode::Pair => 0,
        }
    }
}
