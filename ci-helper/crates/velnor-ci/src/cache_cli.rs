// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    #[command(
        about = "In trusted workflow_run cleanup, validate the completed same-repository PR run and keep its newest delta per lane"
    )]
    TrimDeltas(crate::delta_cli::TrimDeltasArgs),
    #[command(
        about = "Read velnor-run.log in $RUNNER_TEMP and write the step output save=true when the run compiled, checked or documented at least one crate, save=false otherwise; with true, remove <target>/<profile>/incremental and <target>/<triple>/<profile>/incremental, remove the --archive file, write target/.velnor/born with today's date when it is missing, and print the size of --target-dir"
    )]
    Decide(DecideArgs),
    #[command(
        about = "Delete all but the newest entries whose key starts with --prefix on $GITHUB_REF, through the GitHub API; reads GITHUB_TOKEN, which needs actions: write, and GITHUB_REPOSITORY. A failed call prints a warning and exits 0"
    )]
    Trim(TrimArgs),
}

#[derive(Debug, Args)]
pub struct DecideArgs {
    #[arg(long, help = "The build directory the cache saves")]
    pub target_dir: PathBuf,
    #[arg(long, help = "A nextest archive file to remove before the save")]
    pub archive: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct TrimArgs {
    #[arg(long, help = "Key prefix of the family to trim")]
    pub prefix: String,
    #[arg(long, help = "Entries to keep, default 2")]
    pub keep: Option<usize>,
}
