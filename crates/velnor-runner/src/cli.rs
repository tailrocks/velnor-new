// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use uuid::Uuid;

use crate::exec::{AdapterRegistry, JobCancel, JobResult, RunOptions, StdoutSink, run_job};
use crate::job_message::AgentJobRequestMessage;

const USAGE_ERROR: u8 = 2;

#[derive(Debug, Parser)]
#[command(
    name = "velnor-runner",
    version,
    about = "Velnor self-hosted GitHub Actions runner"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Run a saved job message on this machine without GitHub")]
    Exec(ExecArgs),
}

#[derive(Debug, Args)]
pub struct ExecArgs {
    #[arg(long, value_name = "FILE", help = "JSON file with the job message")]
    pub job: PathBuf,
    #[arg(long, value_name = "DIR", help = "Workspace directory of the job")]
    pub workspace: Option<PathBuf>,
    #[arg(long, value_name = "DIR", help = "Runner temp directory of the job")]
    pub temp: Option<PathBuf>,
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("resolve {}", path.display()))
}

fn options(args: &ExecArgs) -> Result<RunOptions> {
    let workspace = match &args.workspace {
        Some(workspace) => absolute(workspace)?,
        None => std::env::temp_dir()
            .join(format!("velnor-exec-{}", Uuid::new_v4()))
            .join("workspace"),
    };
    let temp = match &args.temp {
        Some(temp) => absolute(temp)?,
        None => workspace.with_file_name("temp"),
    };
    Ok(RunOptions {
        workspace,
        temp,
        runner_name: "velnor-exec".to_owned(),
    })
}

async fn exec(args: &ExecArgs) -> Result<ExitCode> {
    let body = std::fs::read_to_string(&args.job)
        .with_context(|| format!("read {}", args.job.display()))?;
    let message = AgentJobRequestMessage::parse_json(&body)?;
    let options = options(args)?;
    let run = run_job(
        &message,
        &options,
        &AdapterRegistry::empty(),
        &mut StdoutSink,
        JobCancel::new(),
    )
    .await;
    Ok(match run.result {
        JobResult::Succeeded => ExitCode::SUCCESS,
        JobResult::Failed | JobResult::Cancelled => ExitCode::FAILURE,
    })
}

fn run(cli: &Cli) -> Result<ExitCode> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("start the async runtime")?;
    match &cli.command {
        Command::Exec(args) => runtime.block_on(exec(args)),
    }
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}
