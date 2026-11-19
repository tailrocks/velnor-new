// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod activity;
mod archive_witness;
mod audit;
mod build_inputs;
mod cache;
pub mod cache_api;
mod cache_paths;
mod cache_sources;
mod cache_upload;
mod cold;
mod configurations;
mod crates_mirror;
mod crates_mirror_dispatch;
mod crates_mirror_mise;
mod git;
mod git_witness;
mod http;
mod inputs;
mod manifests;
mod plan;
pub mod pump;
mod recording;
mod release;
mod report;
mod report_checkout;
mod report_inputs;
mod required;
mod restored;
mod run;
mod setup;
mod timings;
mod unit_platforms;
mod units;

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

pub use self::audit::{AuditOptions, audit_inputs};
pub use self::cache::{Decision, Expiry, decide, expire_old_bases};
pub use self::cold::{cold_on_failure, every_root_target, root_target};
pub use self::inputs::{restore_inputs, save_inputs};
pub use self::restored::{break_time_ties, break_time_ties_with};
pub use self::timings::write_report;
use crate::cli::{
    AuditArgs, BenchCommand, CacheCommand, CheckTagArgs, Cli, CollectArgs, Command, CompareArgs,
    DecideArgs, InputsCommand, ReleaseCommand, ReleaseGateArgs,
};
use crate::collect::collect;
use crate::compare::{compare, compare_across_populations};
use crate::error::CiError;
use crate::escape::stderr_line;
use crate::gate::{check_tag, release_gate};
use crate::github::Repo;
use crate::metrics::{METRICS_SCHEMA_VERSION, RunMetrics};
use clap::{CommandFactory, FromArgMatches};

const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";
const API_VARIABLE: &str = "GITHUB_API_URL";
const OUTPUT_VARIABLE: &str = "GITHUB_OUTPUT";
const SUMMARY_VARIABLE: &str = "GITHUB_STEP_SUMMARY";
const EVENT_VARIABLE: &str = "GITHUB_EVENT_NAME";
const SOURCE_LIMIT: u64 = 1 << 20;
const SOURCE_TOTAL_LIMIT: u64 = 8 << 20;
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
    if crates_mirror::is_wrapper() {
        return crates_mirror::run();
    }
    let cli = match parse_cli(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) if error.use_stderr() => {
            eprintln!("{}", stderr_line(&error.render().to_string()));
            return ExitCode::from(2);
        }
        Err(error) => error.exit(),
    };
    if let Command::Run(args) = &cli.command {
        return run::run(args);
    }
    match run(cli) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", stderr_line(&error.to_string()));
            ExitCode::FAILURE
        }
    }
}

fn run_decide(args: DecideArgs) -> Result<String, CiError> {
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|dir| !dir.is_empty())
        .ok_or_else(|| {
            CiError::new("set RUNNER_TEMP to the directory containing velnor-run.log")
        })?;
    let log_path = Path::new(&temp).join("velnor-run.log");
    let log = fs::read(&log_path)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", log_path.display())))?;
    let target = cache_paths::validated(&current_dir()?, &args.target_dir)?;
    let manifests = cache_paths::list("VELNOR_CACHE_MANIFESTS")?.unwrap_or(args.manifest_path);
    let refusal = cache_sources::refusal(&current_dir()?, &manifests)?;
    let sources = refusal.is_none();
    let decision = if let Some(reason) = refusal {
        Decision {
            save: false,
            report: reason,
        }
    } else {
        let counts = activity::activity()?
            .unwrap_or_else(|| crate::logscan::scan(&String::from_utf8_lossy(&log)));
        cache::decide_counts(&counts, &target, args.archive.as_deref(), now_seconds())?
    };
    append_outputs(&format!("save={}\nsources={sources}\n", decision.save))?;
    Ok(decision.report)
}

fn run(cli: Cli) -> Result<String, CiError> {
    match cli.command {
        Command::Report(args) => report::run_report(&args),
        Command::Bench(BenchCommand::Collect(args)) => run_collect(&args),
        Command::Bench(BenchCommand::Compare(args)) => run_compare(&args),
        Command::Inputs(InputsCommand::Save(args)) => {
            save_inputs(&current_dir()?, &args.target_dir)
        }
        Command::Inputs(InputsCommand::Restore(args)) => {
            let root = current_dir()?;
            cold_on_failure(
                &[cache_paths::validated(&root, &args.target_dir)?],
                "inputs restore",
                || restore_inputs(&root, &args.target_dir),
            )
        }
        Command::Inputs(InputsCommand::Audit(AuditArgs { write })) => audit_inputs(
            &current_dir()?,
            &AuditOptions {
                write,
                ..AuditOptions::default()
            },
        ),
        Command::Inputs(InputsCommand::Build(args)) => audit::build(&current_dir()?, &args),
        Command::ReleaseGate(args) => run_release_gate(&args),
        Command::CheckTag(CheckTagArgs { tag }) => check_tag(env!("CARGO_PKG_VERSION"), &tag),
        Command::Release(ReleaseCommand::Record(args)) => release::run_record(&args),
        Command::Release(ReleaseCommand::Preflight(args)) => release::run_preflight(&args),
        Command::Release(ReleaseCommand::Publish(args)) => release::run_publish(&args),
        Command::Release(ReleaseCommand::CheckConfig(args)) => release::run_check_config(&args),
        Command::Plan(args) => plan::run_plan(&args),
        Command::Gate(args) => required::run_gate(&args),
        Command::Timings(args) => timings::run_timings(&args),
        Command::CratesMirror(args) => crates_mirror::setup(&args),
        Command::MbxStore => crate::mbx_store::setup(),
        Command::Prepare => setup::prepare(),
        Command::CheckMetadata => setup::check_metadata(),
        Command::CheckConfigurations(args) => configurations::check(&args),
        Command::Cache(CacheCommand::Decide(args)) => run_decide(args),
        Command::Cache(CacheCommand::Verify(args)) => cache_upload::verify(&args.key),
        Command::Cache(CacheCommand::Check(args)) => cache_upload::check(&args.success),
        Command::Cache(CacheCommand::Trim(args)) => cache::run_trim(&args.prefix, args.keep),
        Command::Cache(CacheCommand::Budget(args)) => cache_api::run_budget(&args),
        Command::Restored(args) => {
            let root = current_dir()?;
            let configured = cache_paths::list("VELNOR_CACHE_TARGETS")?.unwrap_or(args.target_dir);
            let targets = if configured.is_empty() {
                every_root_target(&root)
            } else {
                configured
                    .iter()
                    .map(|target| cache_paths::validated(&root, target))
                    .collect::<Result<Vec<_>, _>>()?
            };
            let expiry = expire_old_bases(&targets, now_seconds())?;
            append_outputs(&format!("expired={}\n", expiry.expired))?;
            let ties = cold_on_failure(&targets, "restored", || break_time_ties(&root))?;
            Ok(expiry.report + &ties)
        }
        Command::Run(_) => Err(CiError::new("run is handled before commands")),
    }
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

fn current_dir() -> Result<std::path::PathBuf, CiError> {
    std::env::current_dir()
        .map_err(|e| CiError::new(format!("cannot read the working directory: {e}")))
}

fn token() -> Result<String, CiError> {
    std::env::var(TOKEN_VARIABLE)
        .map_err(|_| CiError::new(format!("set {TOKEN_VARIABLE} to a GitHub token")))
}

fn run_collect(args: &CollectArgs) -> Result<String, CiError> {
    let token = token()?;
    let repo = Repo::parse(&args.repo)?;
    let github = http::HttpGitHub::new(api_url(), token, http::DEFAULT_TIMEOUT);
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
    let token = token()?;
    let repo = Repo::parse(&args.repo)?;
    let verdict = release_gate(
        &http::HttpGitHub::new(api_url(), token, http::DEFAULT_TIMEOUT),
        &repo,
        &args.sha,
        &args.pr_only_check,
        std::env::var("GITHUB_RUN_ID")
            .ok()
            .map(|id| id.parse::<u64>())
            .transpose()
            .map_err(|e| CiError::new(format!("invalid GITHUB_RUN_ID: {e}")))?,
    )?;
    append_outputs(&format!("ready={}\n", verdict.ready))?;
    Ok(verdict.report)
}

fn append_outputs(outputs: &str) -> Result<(), CiError> {
    append_to(OUTPUT_VARIABLE, outputs)
}

fn append_to(variable: &str, outputs: &str) -> Result<(), CiError> {
    if let Some(file) = std::env::var_os(variable).filter(|file| !file.is_empty()) {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file)
            .and_then(|mut handle| handle.write_all(outputs.as_bytes()))
            .map_err(|e| CiError::new(format!("cannot write {}: {e}", file.display())))?;
    }
    Ok(())
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
    let a = read_metrics(&args.a)?;
    let b = read_metrics(&args.b)?;
    if args.allow_different_populations {
        compare_across_populations(&a, &b)
    } else {
        compare(&a, &b)
    }
}

pub fn read_metrics(path: &Path) -> Result<RunMetrics, CiError> {
    let text = fs::read_to_string(path)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", path.display())))?;
    let metrics: RunMetrics = serde_json::from_str(&text)
        .map_err(|e| CiError::new(format!("{} is not a metrics file: {e}", path.display())))?;
    if metrics.schema_version != METRICS_SCHEMA_VERSION {
        return Err(CiError::new(format!(
            "{} uses unsupported metrics schema version {}; expected {METRICS_SCHEMA_VERSION}",
            path.display(),
            metrics.schema_version
        )));
    }
    Ok(metrics)
}
