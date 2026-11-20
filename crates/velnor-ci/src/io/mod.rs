// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod activity;
mod archive_witness;
mod artifact_api;
mod artifact_unzip;
mod audit;
mod budget;
mod build_inputs;
mod cache;
pub mod cache_api;
mod cache_paths;
mod cache_prune;
mod cache_sources;
mod cache_upload;
mod cold;
mod configurations;
mod crates_mirror;
mod crates_mirror_args;
mod crates_mirror_dispatch;
mod crates_mirror_mise;
mod delta;
mod delta_freshness;
mod delta_paths;
mod delta_snapshot;
mod delta_source;
mod delta_tree;
mod delta_trim;
mod early_list;
mod fetch;
mod fetch_credentials;
mod fetch_reports;
mod fetch_toolchain;
mod git;
mod git_witness;
mod http;
mod image_candidate;
mod input_scopes;
mod input_snapshot;
mod input_target;
mod inputs;
mod inputs_batch;
mod inputs_git;
mod manifests;
mod merge_group;
mod merge_queue;
mod plan;
mod plan_request;
mod private_check;
pub mod pump;
mod qualification;
mod recording;
mod registry;
mod release;
mod release_align;
mod report;
mod report_checkout;
mod report_inputs;
mod report_sparse;
mod required;
mod resolve_qualification;
mod restored;
mod restored_targets;
mod run;
mod scope_cache;
mod setup;
mod small;
mod sources;
mod timings;
mod unit_platforms;
mod units;
mod write_request;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::{path::Path, process::ExitCode};

pub use self::audit::{AuditOptions, audit_inputs};
pub use self::cache::{Decision, Expiry, decide, expire_old_bases};
pub use self::cold::{cold_on_failure, every_root_target};
pub use self::input_target::{CaseSensitivity, check_input_target_with};
pub use self::inputs::{restore_inputs, save_inputs};
pub use self::inputs_batch::restore_inputs_batch_with;
pub use self::restored::{break_time_ties, break_time_ties_with};
pub use self::scope_cache::ScopeCacheIo;
pub use self::timings::write_report;
use crate::cli::{
    AuditArgs, BenchCommand, CacheCommand, CheckTagArgs, Cli, CollectArgs, Command, CompareArgs,
    ImageCommand, InputsCommand, QualificationCommand, ReleaseCommand, ReleaseGateArgs,
    SourcesCommand, TestGroupsCommand,
};
use crate::collect::collect;
use crate::compare::{compare, compare_across_populations};
use crate::error::CiError;
use crate::escape::stderr_line;
use crate::gate::{check_tag, release_gate};
use crate::github::Repo;
use crate::lane_guard;
use crate::metrics::{METRICS_SCHEMA_VERSION, RunMetrics};
use crate::test_groups;
use clap::{CommandFactory, FromArgMatches};

const TOKEN_VARIABLE: &str = "GITHUB_TOKEN";
const API_VARIABLE: &str = "GITHUB_API_URL";
const OUTPUT_VARIABLE: &str = "GITHUB_OUTPUT";
const SUMMARY_VARIABLE: &str = "GITHUB_STEP_SUMMARY";
const EVENT_VARIABLE: &str = "GITHUB_EVENT_NAME";
const SOURCE_LIMIT: u64 = 1 << 20;
const SOURCE_TOTAL_LIMIT: u64 = 64 << 20;
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
    if let Command::Sources(SourcesCommand::Fetch(args)) = &cli.command {
        return sources::fetch(args);
    }
    if let Command::LaneGuard(args) = &cli.command {
        return lane_guard::run(args);
    }
    if let Command::TestGroups(TestGroupsCommand::Check(args)) = &cli.command {
        return test_groups::run(args);
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

fn run(cli: Cli) -> Result<String, CiError> {
    match cli.command {
        Command::Delta(command) => delta::run(command),
        Command::Report(args) => report::run_report(&args),
        Command::Bench(BenchCommand::Collect(args)) => run_collect(&args),
        Command::Bench(BenchCommand::Compare(args)) => run_compare(&args),
        Command::Bench(BenchCommand::Budget(args)) => budget::run_budget(&args),
        Command::Inputs(InputsCommand::Save(args)) => {
            inputs_batch::save_args(&current_dir()?, &args)
        }
        Command::Inputs(InputsCommand::Restore(args)) => {
            let root = current_dir()?;
            input_target::check_target(&root, &args.target_dir)?;
            restored_targets::record(&root, &args.target_dir)?;
            cold_on_failure(&[root.join(&args.target_dir)], "inputs restore", || {
                restore_inputs(&root, &args.target_dir)
            })
        }
        Command::Inputs(InputsCommand::RestoreAll(args)) => {
            inputs_batch::restore_args(&current_dir()?, &args)
        }
        Command::Inputs(InputsCommand::Audit(AuditArgs { write, root })) => audit_inputs(
            &current_dir()?,
            &AuditOptions {
                write,
                root,
                ..AuditOptions::default()
            },
        ),
        Command::Inputs(InputsCommand::Build(args)) => audit::build(&current_dir()?, &args),
        Command::ReleaseGate(args) => run_release_gate(&args),
        Command::CheckTag(CheckTagArgs { tag }) => check_tag(env!("CARGO_PKG_VERSION"), &tag),
        Command::Release(ReleaseCommand::Record(args)) => release::run_record(&args),
        Command::Release(ReleaseCommand::Preflight(args)) => release::run_preflight(&args),
        Command::Release(ReleaseCommand::Publish(args)) => release::run_publish(&args),
        Command::Release(ReleaseCommand::FamilyTag(args)) => release::run_family_tag(&args),
        Command::Release(ReleaseCommand::CheckConfig(args)) => release::run_check_config(&args),
        Command::Release(ReleaseCommand::Align(args)) => release_align::run_align(&args),
        Command::Plan(args) => plan::run_plan(&args),
        Command::MergeGroupPassed => Ok(merge_group::run()),
        Command::MergeQueueCheck => Ok(merge_queue::run()),
        Command::WriteRequest(args) => write_request::run(&args),
        Command::ResolveQualification(args) => resolve_qualification::run(&args),
        Command::Gate(args) => required::run_gate(&args),
        Command::Qualification(QualificationCommand::Compare(args)) => {
            qualification::run_compare(&args)
        }
        Command::FetchReports(args) => fetch_reports::run_fetch_reports(&args),
        Command::Timings(args) => timings::run_timings(&args),
        Command::CratesMirror(args) => crates_mirror::setup(&args),
        Command::Image(ImageCommand::VerifyCandidate(args)) => {
            image_candidate::verify_candidate(&args)
        }
        Command::Fetch(args) => fetch::fetch(&args),
        Command::PrivateTokenAllowed => private_check::run(),
        Command::MbxStore => crate::mbx_store::setup(),
        Command::MbxSeed => crate::mbx_seed::setup(),
        Command::ToolsLocal(args) => crate::tools_local::setup(args.verb, &args.key),
        Command::Prepare(args) => setup::prepare(&args),
        Command::CheckMetadata => setup::check_metadata(),
        Command::CheckConfigurations(args) => configurations::check(&args),
        Command::Cache(CacheCommand::Decide(args)) => cache::run_decide(&args),
        Command::Cache(CacheCommand::Verify(args)) => cache_upload::verify(&args.key),
        Command::Cache(CacheCommand::Check(args)) => cache_upload::check(&args.success),
        Command::Cache(CacheCommand::Trim(args)) => cache::run_trim(&args.prefix, args.keep),
        Command::Cache(CacheCommand::TrimDeltas(args)) => delta_trim::run(args.run_id),
        Command::Cache(CacheCommand::Budget(args)) => cache_api::run_budget(&args),
        Command::Restored(args) => small::run_restored(&args),
        Command::ValidateArtifactId => small::run_validate_artifact_id(),
        Command::Run(_) | Command::Sources(_) => {
            Err(CiError::new("run and sources are handled before commands"))
        }
        Command::LaneGuard(_) => Err(CiError::new("lane-guard is handled before commands")),
        Command::TestGroups(_) => Err(CiError::new("test-groups is handled before commands")),
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

pub(super) fn open_regular(path: &Path) -> Option<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    file.metadata().ok()?.is_file().then_some(file)
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
