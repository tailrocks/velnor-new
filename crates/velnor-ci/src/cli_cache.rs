// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    #[command(
        about = "In trusted workflow_run cleanup, validate the completed same-repository PR run and keep its newest delta per lane"
    )]
    TrimDeltas(crate::delta_cli::TrimDeltasArgs),
    #[command(
        about = "Read velnor-run.log in $RUNNER_TEMP and write the step output save=true when the run compiled, checked or documented at least one crate, save=false otherwise or when any checkout Cargo.lock has dependency sources not proven public; a missing or unreadable run log is an error; with true, remove <target>/<profile>/incremental and <target>/<triple>/<profile>/incremental, remove the --archive file, write target/.velnor/born with today's date when it is missing, and print the size of --target-dir"
    )]
    Decide(DecideArgs),
    #[command(
        about = "Delete all but the newest entries whose key starts with --prefix on $GITHUB_REF, through the GitHub API; reads GITHUB_TOKEN, which needs actions: write, and GITHUB_REPOSITORY. A missing token or a failed API call exits with an error"
    )]
    Trim(TrimArgs),
    #[command(about = "Require an exact saved cache entry through the read-only cache API")]
    Verify(VerifyArgs),
    #[command(about = "Fail the Cache check when a lane could not verify its uploads")]
    Check(CheckArgs),
    #[command(about = "Enforce the repository cache budget while protecting newest bases")]
    Budget(BudgetArgs),
}

#[derive(Debug, Args)]
pub struct BudgetArgs {
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, help = "Repository as owner/name, default GITHUB_REPOSITORY")]
    pub repo: Option<String>,
    #[arg(long, default_value_t = velnor_rust::DEFAULT_CACHE_BUDGET_BYTES, value_parser = clap::value_parser!(u64).range(0..=velnor_rust::DEFAULT_CACHE_BUDGET_BYTES))]
    pub budget_bytes: u64,
}

#[derive(Debug, Args)]
pub struct DecideArgs {
    #[arg(
        long,
        default_value = "Cargo.toml",
        help = "Detected Cargo root manifests contributing to this job"
    )]
    pub manifest_path: Vec<PathBuf>,
    #[arg(
        long,
        required_unless_present = "paths_env",
        conflicts_with = "paths_env",
        help = "The build directory the cache saves"
    )]
    pub target_dir: Option<PathBuf>,
    #[arg(
        long,
        conflicts_with = "paths_env",
        help = "A nextest archive file to remove before the save"
    )]
    pub archive: Option<PathBuf>,
    #[arg(
        long,
        help = "Environment variable containing JSON [hex target, optional hex archive] paths"
    )]
    pub paths_env: Option<String>,
}

#[derive(Debug, Args)]
pub struct TrimArgs {
    #[arg(long, help = "Key prefix of the family to trim")]
    pub prefix: String,
    #[arg(long, help = "Entries to keep, default 2")]
    pub keep: Option<usize>,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[arg(long)]
    pub key: String,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(long)]
    pub success: String,
}
