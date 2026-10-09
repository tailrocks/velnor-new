//! Typed dispatch for public commands and the private file entrypoint.
//!
//! The private gate requires `VELNOR_INTERNAL_OP` plus the exact request file
//! in `VELNOR_REQUEST_FILE`: `write-request-v1` needs GitHub event env and no
//! pre-existing file, `plan-v1`/`merge-v1`/`publish-baseline-v1` need a
//! pre-existing request file, `fetch-reports-v1`/`write-task-report-v1`
//! need runner temp plus the numeric run ID instead, and
//! `write-preseed-manifest-v1` needs runner temp only. Anything else falls
//! through to Clap, so public behavior is byte-identical with or without
//! the environment set.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_actions_contract_workflow::{DYNAMIC_MATRIX_OUTPUT_MODE, PLAN_MATRIX_OUTPUT_MODE_ENV};
use velnor_actions_orchestrator_check_runtime::execute_check;
use velnor_actions_orchestrator_core::{OrchestratorError, init_config, resolve_root};
use velnor_actions_orchestrator_generation::generate::{GenerateOptions, generate_dispatched};
use velnor_actions_orchestrator_generation::prepare::prepare;
use velnor_actions_orchestrator_generation::routing::parse_dispatch_mode;
use velnor_actions_orchestrator_internal::artifact_export::materialize_artifact_from_environment;
use velnor_actions_orchestrator_internal::internal::{
    merge_passed, plan_internal, plan_outputs, publish_final_report, publish_plan_files,
    response_path_for, write_request,
};
use velnor_actions_orchestrator_internal::merge_entry::merge_internal;
use velnor_actions_orchestrator_plan::plan::plan_text_checked;
use velnor_actions_orchestrator_plan::plan_output_limits::PlanOutputMode;
use velnor_actions_orchestrator_preseed_manifest::write_preseed_manifest;
use velnor_actions_orchestrator_retrieve_reports::retrieve_reports;
use velnor_actions_orchestrator_task_report_write::write_task_report;

use crate::args::{Cli, Command};
use crate::dispatch_gate::{InternalOp, InternalRequest, gate_request};
use crate::dispatch_publish::run_publish_internal;
use crate::dispatch_verification_artifact::run_verification_artifact_internal;

/// Environment variable carrying the `$GITHUB_OUTPUT` path. Never printed.
const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
/// Environment variable carrying the runner temp dir. Never printed.
pub(crate) const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

/// Parse Clap arguments and dispatch one public command.
///
/// Clap owns `--help`, `--version`, and usage errors (exit 2).
pub(crate) fn run_public() -> ExitCode {
    match Cli::parse().command {
        Command::Init => run_init(),
        Command::Plan => run_plan(),
        Command::Generate { output_dir, mode } => run_generate(output_dir, mode),
        Command::Config { command } => crate::dispatch_config::run_config(&command),
    }
}

/// Run the private file entrypoint when the gate is satisfied.
///
/// Returns `None` when the gate is not satisfied so the caller falls through
/// to Clap; bare invocations then fail with the usage diagnostic (exit 2),
/// keeping public behavior byte-identical with or without the environment.
pub(crate) fn try_internal() -> Option<ExitCode> {
    let request = gate_request()?;
    Some(run_internal(&request))
}

/// Run one validated private operation.
fn run_internal(request: &InternalRequest) -> ExitCode {
    match request.op {
        InternalOp::ExecuteCheck => match execute_check() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::WriteRequest => match write_request() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::Plan => run_plan_internal(&request.path),
        InternalOp::Merge => run_merge_internal(&request.path),
        InternalOp::Fetch => match retrieve_reports() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::ArtifactExport => match materialize_artifact_from_environment(&request.path) {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::VerificationArtifactExport => run_verification_artifact_internal(&request.path),
        InternalOp::Report => match write_task_report() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::PreseedManifest => match write_preseed_manifest() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::Publish => run_publish_internal(&request.path),
    }
}

/// Read the request, run the planner, publish files, write response plus outputs.
fn run_plan_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let mode = match env::var_os(PLAN_MATRIX_OUTPUT_MODE_ENV) {
        None => PlanOutputMode::Static,
        Some(value) if value == DYNAMIC_MATRIX_OUTPUT_MODE => PlanOutputMode::DynamicMatrix,
        Some(_) => return fail_internal("bad_plan_matrix_output_mode"),
    };
    let response = match plan_internal(&text) {
        Ok(response) => response,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return fail_internal(&error.to_string()),
    };
    if let Err(error) = fs::write(&sibling, &response) {
        return fail_internal(&format!("write response: {error}"));
    }
    let outputs = match plan_outputs(&response, mode) {
        Ok(outputs) => outputs,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    if let Err(error) = publish_plan_files(&response, &Path::new(&runner_temp).join("velnor")) {
        return fail_internal(&error.to_string());
    }
    let mut body = String::new();
    for (name, value) in outputs.step_outputs() {
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    match fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&output_path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(body.as_bytes())
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append outputs: {error}")),
    }
}

/// Read the request, run the merge, publish the verdict, exit verdict.
fn run_merge_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let response = match merge_internal(&text) {
        Ok(response) => response,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return fail_internal(&error.to_string()),
    };
    if let Err(error) = fs::write(&sibling, &response) {
        return fail_internal(&format!("write response: {error}"));
    }
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    if let Err(error) = publish_final_report(&response, &Path::new(&runner_temp).join("velnor")) {
        return fail_internal(&error.to_string());
    }
    match merge_passed(&response) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => fail_internal(&error.to_string()),
    }
}

/// Report a private failure without printing the private operation.
fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}

/// Dispatch `init`: resolve the root, then create the config file.
fn run_init() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let report = resolve_root(&cwd).and_then(|root| init_config(&root));
    match report {
        Ok(report) => {
            for path in &report.created {
                println!("{path}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}

/// Dispatch `plan`: report (recommendations included) to stdout only.
///
/// Contract §5 routes findings to stderr in one sentence, but §5's own
/// example shows `Recommendations` inside the stdout report and §7 assigns
/// the report to stdout; the example plus §7 govern, so stderr stays empty.
fn run_plan() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let preparation = resolve_root(&cwd).and_then(|root| prepare(&root));
    let preparation = match preparation {
        Ok(preparation) => preparation,
        Err(error) => return fail_public(&error),
    };
    let text = match plan_text_checked(&preparation) {
        Ok(text) => text,
        Err(error) => return fail_public(&error),
    };
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
    ExitCode::SUCCESS
}

/// Dispatch `generate`: files written and recommendations go to stderr.
fn run_generate(output_dir: Option<PathBuf>, mode: Option<String>) -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let options = GenerateOptions { output_dir };
    let root = match resolve_root(&cwd) {
        Ok(root) => root,
        Err(error) => return fail_public(&error),
    };
    let preparation = match prepare(&root) {
        Ok(preparation) => preparation,
        Err(error) => return fail_public(&error),
    };
    let dispatch = match mode {
        Some(text) => match parse_dispatch_mode(&text) {
            Ok(mode) => Some(mode),
            Err(error) => return fail_public(&error),
        },
        None => None,
    };
    match generate_dispatched(&preparation, &options, dispatch) {
        Ok(report) => {
            if let Some(dir) = &options.output_dir {
                eprintln!("Preview: {}", absolute_preview(&cwd, dir).display());
                eprintln!("Repository: {}", root.display());
            }
            for path in &report.files_written {
                eprintln!("{path}");
            }
            for recommendation in &report.recommendations {
                eprintln!("{recommendation}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}

/// Absolute preview path for the stderr report; canonical when possible.
fn absolute_preview(cwd: &Path, dir: &Path) -> PathBuf {
    let joined = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    };
    joined.canonicalize().unwrap_or(joined)
}

/// Read the working directory, reporting failures as exit 1.
pub(crate) fn working_dir() -> Option<PathBuf> {
    match env::current_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            eprintln!("velnor-actions: working directory: {error}");
            None
        }
    }
}

/// Report an orchestrator failure as exit 1.
pub(crate) fn fail_public(error: &OrchestratorError) -> ExitCode {
    eprintln!("velnor-actions: {}", single_line(&error.to_string()));
    ExitCode::from(1)
}

/// Collapse one error to a single log line (X8: values echoed into
/// errors, such as `unsupported_label` or `bad_custom_task`, must not
/// inject newlines into logs).
fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
