//! Typed dispatch for public commands and the private file entrypoint.
//!
//! The private gate requires `VELNOR_INTERNAL_OP` plus the exact request file
//! in `VELNOR_REQUEST_FILE`: `write-request-v1` needs GitHub event env and no
//! pre-existing file, `plan-v1`/`merge-v1` need a pre-existing request file.
//! Anything else falls through to Clap, so public behavior is byte-identical
//! with or without the environment set.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_actions_orchestrator::{
    GenerateOptions, MERGE_OP, OrchestratorError, PLAN_OP, REQUEST_FILE_ENV, WRITE_REQUEST_OP,
    generate, init_config, merge_internal, merge_passed, plan_internal, plan_outputs, plan_text,
    prepare, resolve_root, response_path_for, write_request,
};

use crate::args::{Cli, Command};

/// Environment variable selecting the private operation. Never printed.
const OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Environment variable carrying the `$GITHUB_OUTPUT` path. Never printed.
const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
/// Environment variable carrying the triggering event name. Never printed.
const GITHUB_EVENT_ENV: &str = "GITHUB_EVENT_NAME";
/// Environment variable carrying the event payload path. Never printed.
const GITHUB_EVENT_PATH_ENV: &str = "GITHUB_EVENT_PATH";

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalOp {
    /// Materialize the request file from the GitHub environment.
    WriteRequest,
    /// Plan operation.
    Plan,
    /// Merge operation.
    Merge,
}

/// Validated private request: operation plus exact request-file path.
#[derive(Debug)]
struct InternalRequest {
    /// Operation to run.
    op: InternalOp,
    /// Exact request-file path from the environment.
    path: PathBuf,
}

/// Parse Clap arguments and dispatch one public command.
///
/// Clap owns `--help`, `--version`, and usage errors (exit 2).
pub(crate) fn run_public() -> ExitCode {
    match Cli::parse().command {
        Command::Init => run_init(),
        Command::Plan => run_plan(),
        Command::Generate { output_dir } => run_generate(output_dir),
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

/// Check the private gate: known op plus request-file presence by op.
fn gate_request() -> Option<InternalRequest> {
    let op = match env::var(OP_ENV).as_deref() {
        Ok(tag) if tag == WRITE_REQUEST_OP => InternalOp::WriteRequest,
        Ok(tag) if tag == PLAN_OP => InternalOp::Plan,
        Ok(tag) if tag == MERGE_OP => InternalOp::Merge,
        _ => return None,
    };
    let path = env::var_os(REQUEST_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    match op {
        InternalOp::WriteRequest => {
            if path.exists() {
                return None;
            }
            if !env::var(GITHUB_EVENT_ENV).is_ok_and(|name| !name.is_empty()) {
                return None;
            }
            if env::var_os(GITHUB_EVENT_PATH_ENV).is_none_or(|value| value.is_empty()) {
                return None;
            }
        }
        InternalOp::Plan | InternalOp::Merge => {
            if !path.is_file() {
                return None;
            }
        }
    }
    Some(InternalRequest { op, path })
}

/// Run one validated private operation.
fn run_internal(request: &InternalRequest) -> ExitCode {
    match request.op {
        InternalOp::WriteRequest => match write_request() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::Plan => run_plan_internal(&request.path),
        InternalOp::Merge => run_merge_internal(&request.path),
    }
}

/// Read the request, run the planner, write the sibling response plus outputs.
fn run_plan_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
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
    let outputs = match plan_outputs(&response) {
        Ok(outputs) => outputs,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    let body = format!("matrix={}\nplan={}\n", outputs.matrix, outputs.plan);
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

/// Read the request, run the merge, write the sibling response, exit verdict.
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
    let text = plan_text(&preparation);
    print!("{text}");
    if !text.ends_with('\n') {
        println!();
    }
    ExitCode::SUCCESS
}

/// Dispatch `generate`: files written and recommendations go to stderr.
fn run_generate(output_dir: Option<PathBuf>) -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let options = GenerateOptions { output_dir };
    let root = match resolve_root(&cwd) {
        Ok(root) => root,
        Err(error) => return fail_public(&error),
    };
    let report = prepare(&root).and_then(|preparation| generate(&preparation, &options));
    match report {
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
fn working_dir() -> Option<PathBuf> {
    match env::current_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            eprintln!("velnor-actions: working directory: {error}");
            None
        }
    }
}

/// Report an orchestrator failure as exit 1.
fn fail_public(error: &OrchestratorError) -> ExitCode {
    eprintln!("velnor-actions: {error}");
    ExitCode::from(1)
}
