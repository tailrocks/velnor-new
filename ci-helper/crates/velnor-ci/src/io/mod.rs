// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod audit;
mod http;
mod inputs;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};
use velnor_gen::io::{Checkout, read_checkout, read_files};
use velnor_model::{ChangeSet, RepoPath};
use velnor_rust::{cargo_files, detect, optional_steps, root_sources, select, unknown_paths};

pub use self::audit::audit_inputs;
pub use self::inputs::{restore_inputs, save_inputs};
use crate::cli::{
    AuditArgs, BaseFile, BenchCommand, CheckTagArgs, Cli, CollectArgs, Command, CompareArgs,
    InputsCommand, PlanArgs, ReleaseGateArgs,
};
use crate::collect::collect;
use crate::compare::compare;
use crate::error::CiError;
use crate::gate::{check_tag, release_gate};
use crate::github::Repo;
use crate::metrics::RunMetrics;
use crate::plan::{Steps, explain, outputs};

const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";
const API_VARIABLE: &str = "GITHUB_API_URL";
const OUTPUT_VARIABLE: &str = "GITHUB_OUTPUT";
const EVENT_VARIABLE: &str = "GITHUB_EVENT_NAME";
const SCHEDULED: &str = "schedule";
const DEFAULT_API: &str = "https://api.github.com";
const UNKNOWN_COMMIT: &str = "unknown";

#[must_use]
pub fn version_text() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("VELNOR_COMMIT").unwrap_or(UNKNOWN_COMMIT)
    )
}

pub fn parse_cli<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let matches = Cli::command()
        .version(version_text())
        .try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

#[must_use]
pub fn main() -> ExitCode {
    let cli = match parse_cli(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match run(cli) {
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
        Command::Inputs(InputsCommand::Save) => save_inputs(&current_dir()?),
        Command::Inputs(InputsCommand::Restore) => restore_inputs(&current_dir()?),
        Command::Inputs(InputsCommand::Audit(AuditArgs { write })) => {
            audit_inputs(&current_dir()?, write)
        }
        Command::ReleaseGate(args) => run_release_gate(&args),
        Command::CheckTag(CheckTagArgs { tag }) => check_tag(env!("CARGO_PKG_VERSION"), &tag),
        Command::Plan(args) => run_plan(&args),
    }
}

fn run_plan(args: &PlanArgs) -> Result<String, CiError> {
    let checkout = read_checkout(&current_dir()?).map_err(|e| CiError::new(e.to_string()))?;
    let facts = detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?;
    let paths: BTreeSet<RepoPath> = args.changed.iter().cloned().collect();
    let changes = ChangeSet {
        base_files: read_base_files(&args.base_files)?,
        head_files: read_head_files(&checkout, &paths)?,
        paths,
    };
    let selections = select(&facts, &changes);
    let full = std::env::var(EVENT_VARIABLE).is_ok_and(|event| event == SCHEDULED);
    let steps = facts
        .rust_roots
        .iter()
        .map(|root| {
            let paths = root_sources(&facts, root, &checkout.view.tracked_paths);
            let sources =
                read_files(&checkout.root, &paths).map_err(|e| CiError::new(e.to_string()))?;
            Ok((root.manifest.clone(), optional_steps(root, &sources, full)))
        })
        .collect::<Result<Steps, CiError>>()?;
    let outputs = outputs(&selections, &steps)?;
    if let Some(file) = std::env::var_os(OUTPUT_VARIABLE).filter(|file| !file.is_empty()) {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file)
            .and_then(|mut handle| handle.write_all(outputs.as_bytes()))
            .map_err(|e| CiError::new(format!("cannot write {}: {e}", file.display())))?;
    }
    Ok(if args.explain {
        explain(&selections, &steps, &unknown_paths(&facts, &changes))
    } else {
        outputs
    })
}

fn read_base_files(files: &[BaseFile]) -> Result<BTreeMap<RepoPath, String>, CiError> {
    files
        .iter()
        .map(|base| {
            fs::read_to_string(&base.file)
                .map(|text| (base.path.clone(), text))
                .map_err(|e| CiError::new(format!("cannot read {}: {e}", base.file.display())))
        })
        .collect()
}

fn read_head_files(
    checkout: &Checkout,
    changed: &BTreeSet<RepoPath>,
) -> Result<BTreeMap<RepoPath, String>, CiError> {
    let wanted: Vec<RepoPath> = cargo_files(&checkout.view.tracked_paths)
        .into_iter()
        .filter(|path| changed.contains(path))
        .collect();
    let files = read_files(&checkout.root, &wanted).map_err(|e| CiError::new(e.to_string()))?;
    Ok(files
        .into_iter()
        .filter_map(|(path, bytes)| Some((path, String::from_utf8(bytes).ok()?)))
        .collect())
}

fn current_dir() -> Result<std::path::PathBuf, CiError> {
    std::env::current_dir()
        .map_err(|e| CiError::new(format!("cannot read the working directory: {e}")))
}

fn run_collect(args: &CollectArgs) -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .map_err(|_| CiError::new(format!("set {TOKEN_VARIABLE} to a GitHub token")))?;
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token);
    let metrics = collect(&github, &repo, args.run_id, args.allow_rerun)?;
    if let Some(path) = &args.output {
        write_metrics(path, &metrics)?;
    }
    Ok(metrics.summary())
}

fn api_url() -> String {
    std::env::var(API_VARIABLE).unwrap_or_else(|_| DEFAULT_API.to_owned())
}

fn run_release_gate(args: &ReleaseGateArgs) -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .map_err(|_| CiError::new(format!("set {TOKEN_VARIABLE} to a GitHub token")))?;
    let repo = Repo::parse(&args.repo)?;
    release_gate(&http::HttpGitHub::new(api_url(), token), &repo, &args.sha)
}

pub fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, CiError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| CiError::new(e.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(CiError::new(format!(
            "response is larger than {limit} bytes"
        )));
    }
    Ok(bytes)
}

pub fn write_metrics(path: &Path, metrics: &RunMetrics) -> Result<(), CiError> {
    let json = serde_json::to_string_pretty(metrics)
        .map_err(|e| CiError::new(format!("cannot encode metrics: {e}")))?;
    fs::write(path, json + "\n")
        .map_err(|e| CiError::new(format!("cannot write {}: {e}", path.display())))
}

fn run_compare(args: &CompareArgs) -> Result<String, CiError> {
    compare(&read_metrics(&args.a)?, &read_metrics(&args.b)?)
}

pub fn read_metrics(path: &Path) -> Result<RunMetrics, CiError> {
    let text = fs::read_to_string(path)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| CiError::new(format!("{} is not a metrics file: {e}", path.display())))
}
