// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "velnor-ci",
    about = "Measures CI runs and compares the measurements"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(subcommand, about = "Collect and compare run metrics")]
    Bench(BenchCommand),
    #[command(
        subcommand,
        about = "Keep tracked files' modification times with a cached target directory"
    )]
    Inputs(InputsCommand),
    #[command(
        about = "Exit 1 unless the commit is on the default branch and its check Required passed; reads GITHUB_TOKEN"
    )]
    ReleaseGate(ReleaseGateArgs),
    #[command(about = "Exit 1 unless the tag is rust-ci-v followed by the version of this program")]
    CheckTag(CheckTagArgs),
}

#[derive(Debug, Args)]
pub struct CheckTagArgs {
    #[arg(help = "The release tag")]
    pub tag: String,
}

#[derive(Debug, Args)]
pub struct ReleaseGateArgs {
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(long, help = "Full SHA of the commit that is about to be released")]
    pub sha: String,
}

#[derive(Debug, Subcommand)]
pub enum InputsCommand {
    #[command(
        about = "Write target/.velnor/inputs.tsv: path, blob id of the bytes on disk, modification time in nanoseconds and mode of every regular tracked file in the worktree; paths a TSV field cannot hold are counted and left out"
    )]
    Save,
    #[command(
        about = "Set the saved modification time on every tracked file whose path, bytes on disk and mode equal the saved ones; report changed, new, deleted, absent and left-out files"
    )]
    Restore,
}

#[derive(Debug, Subcommand)]
pub enum BenchCommand {
    #[command(about = "Turn a workflow run into a metrics file; reads GITHUB_TOKEN")]
    Collect(CollectArgs),
    #[command(about = "Print a markdown table comparing two metrics files")]
    Compare(CompareArgs),
}

#[derive(Debug, Args)]
pub struct CollectArgs {
    #[arg(help = "Workflow run id")]
    pub run_id: u64,
    #[arg(long, help = "Repository as owner/name")]
    pub repo: String,
    #[arg(short, long, help = "Write the metrics file here")]
    pub output: Option<PathBuf>,
    #[arg(long, help = "Accept a run whose run_attempt is greater than 1")]
    pub allow_rerun: bool,
}

#[derive(Debug, Args)]
pub struct CompareArgs {
    #[arg(help = "Metrics file of the first run")]
    pub a: PathBuf,
    #[arg(help = "Metrics file of the second run")]
    pub b: PathBuf,
}
