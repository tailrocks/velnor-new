// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod http;

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::{BenchCommand, Cli, CollectArgs, Command, CompareArgs};
use crate::collect::collect;
use crate::compare::compare;
use crate::error::CiError;
use crate::github::Repo;
use crate::metrics::RunMetrics;

const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";

#[must_use]
pub fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-ci: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<String, CiError> {
    match cli.command {
        Command::Bench(BenchCommand::Collect(args)) => run_collect(&args),
        Command::Bench(BenchCommand::Compare(args)) => run_compare(&args),
    }
}

fn run_collect(args: &CollectArgs) -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .map_err(|_| CiError::new(format!("set {TOKEN_VARIABLE} to a GitHub token")))?;
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(token);
    let metrics = collect(&github, &repo, args.run_id, args.allow_rerun)?;
    if let Some(path) = &args.output {
        let json = serde_json::to_string_pretty(&metrics)
            .map_err(|e| CiError::new(format!("cannot encode metrics: {e}")))?;
        fs::write(path, json + "\n")
            .map_err(|e| CiError::new(format!("cannot write {}: {e}", path.display())))?;
    }
    Ok(metrics.summary())
}

fn run_compare(args: &CompareArgs) -> Result<String, CiError> {
    Ok(compare(&read_metrics(&args.a)?, &read_metrics(&args.b)?))
}

fn read_metrics(path: &Path) -> Result<RunMetrics, CiError> {
    let text = fs::read_to_string(path)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| CiError::new(format!("{} is not a metrics file: {e}", path.display())))
}
