// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use tokio::signal::unix::{SignalKind, signal};
use uuid::Uuid;

use crate::exec::{AdapterRegistry, JobCancel, JobResult, RunOptions, StdoutSink, run_job};
use crate::job_message::AgentJobRequestMessage;
use crate::lifecycle::{RunJitOptions, run_jit};

const USAGE_ERROR: u8 = 2;
const LIFECYCLE_ERROR: u8 = 1;
const RENEW_INTERVAL: Duration = Duration::from_secs(60);
const CANCEL_POLL_ERROR_DELAY: Duration = Duration::from_secs(2);

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
    #[command(about = "Wait for one job from GitHub with a JIT configuration, run it and exit")]
    Run(RunArgs),
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[arg(
        long,
        value_name = "VALUE",
        help = "The encoded_jit_config string, or @FILE to read it from a file"
    )]
    pub jit: String,
    #[arg(
        long,
        value_name = "DIR",
        default_value = "_work",
        help = "Directory for the job workspace and temp files"
    )]
    pub work_dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct ExecArgs {
    #[arg(long, value_name = "FILE", help = "JSON file with the job message")]
    pub job: PathBuf,
    #[arg(
        long,
        value_name = "DIR",
        help = "Workspace directory of the job (default: a fresh directory removed after the run)"
    )]
    pub workspace: Option<PathBuf>,
    #[arg(
        long,
        value_name = "DIR",
        help = "Runner temp directory of the job (default: a fresh directory removed after the run)"
    )]
    pub temp: Option<PathBuf>,
}

fn absolute(path: &Path) -> Result<PathBuf> {
    std::path::absolute(path).with_context(|| format!("resolve {}", path.display()))
}

struct Scratch(Option<PathBuf>);

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(root) = &self.0
            && let Err(error) = std::fs::remove_dir_all(root)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("warning: remove {}: {error}", root.display());
        }
    }
}

fn options(args: &ExecArgs) -> Result<(RunOptions, Scratch)> {
    let root = std::env::temp_dir().join(format!("velnor-exec-{}", Uuid::new_v4()));
    let scratch = Scratch((args.workspace.is_none() || args.temp.is_none()).then(|| root.clone()));
    let workspace = match &args.workspace {
        Some(workspace) => absolute(workspace)?,
        None => root.join("workspace"),
    };
    let temp = match &args.temp {
        Some(temp) => absolute(temp)?,
        None => root.join("temp"),
    };
    let options = RunOptions {
        workspace,
        temp,
        runner_name: "velnor-exec".to_owned(),
    };
    Ok((options, scratch))
}

fn cancel_on_signal() -> Result<JobCancel> {
    let mut terminate = signal(SignalKind::terminate()).context("listen for SIGTERM")?;
    let mut interrupt = signal(SignalKind::interrupt()).context("listen for SIGINT")?;
    let cancel = JobCancel::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
        trigger.cancel();
    });
    Ok(cancel)
}

async fn exec(args: &ExecArgs) -> Result<ExitCode> {
    let body = std::fs::read_to_string(&args.job)
        .with_context(|| format!("read {}", args.job.display()))?;
    let message = AgentJobRequestMessage::parse_json(&body)?;
    let (options, _scratch) = options(args)?;
    let run = run_job(
        &message,
        &options,
        &build_adapters(),
        &mut StdoutSink,
        cancel_on_signal()?,
    )
    .await;
    Ok(match run.result {
        JobResult::Succeeded => ExitCode::SUCCESS,
        JobResult::Failed | JobResult::Cancelled => ExitCode::FAILURE,
    })
}

fn build_adapters() -> AdapterRegistry {
    AdapterRegistry::empty()
}

fn jit_value(raw: &str) -> Result<String> {
    match raw.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path)
            .map(|text| text.trim().to_owned())
            .with_context(|| format!("read {path}")),
        None => Ok(raw.to_owned()),
    }
}

async fn run_command(args: &RunArgs) -> Result<ExitCode> {
    let options = RunJitOptions {
        jit: jit_value(&args.jit)?,
        work_dir: args.work_dir.clone(),
        renew_interval: RENEW_INTERVAL,
        cancel_poll_error_delay: CANCEL_POLL_ERROR_DELAY,
        adapters: build_adapters(),
    };
    Ok(match run_jit(options).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(LIFECYCLE_ERROR)
        }
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
        Command::Run(args) => runtime.block_on(run_command(args)),
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
