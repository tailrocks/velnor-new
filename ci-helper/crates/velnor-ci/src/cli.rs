// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "velnor-ci",
    version,
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
}

#[derive(Debug, Subcommand)]
pub enum InputsCommand {
    #[command(
        about = "Write target/.velnor/inputs.tsv: path, blob id and modification time in nanoseconds of every tracked file"
    )]
    Save,
    #[command(
        about = "Set the modification time of every tracked file whose blob id is unchanged to the saved value"
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
