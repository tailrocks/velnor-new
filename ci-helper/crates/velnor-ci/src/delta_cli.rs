// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum DeltaCommand {
    #[command(about = "Fingerprint BASE_CACHE_KEY, the target restore's cache-matched-key")]
    Prepare(DeltaArgs),
    #[command(
        about = "Overlay the restored PR delta, preserving times, and record the build marker"
    )]
    Apply(DeltaArgs),
    #[command(
        about = "Stage the cumulative PR delta, preserving times; save=false above 500 MB or without compilation"
    )]
    Stage(DeltaArgs),
}

#[derive(Debug, Args)]
pub struct DeltaArgs {
    #[arg(long, help = "Target directory")]
    pub target_dir: PathBuf,
    #[arg(long, help = "Directory holding the PR delta")]
    pub staging_dir: PathBuf,
    #[arg(
        long = "package",
        help = "Workspace crate whose compilation permits saving; repeat for each crate"
    )]
    pub packages: Vec<String>,
}

#[derive(Debug, Args)]
pub struct TrimDeltasArgs {
    #[arg(long, help = "Completed Rust CI workflow run id")]
    pub run_id: u64,
}
