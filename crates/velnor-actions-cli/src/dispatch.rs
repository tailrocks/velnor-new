//! Typed dispatch for public commands and the private file entrypoint.
//!
//! The private gate requires both `VELNOR_INTERNAL_OP` and a pre-existing
//! request file. Anything else falls through to Clap, so public behavior is
//! byte-identical with or without the environment set.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use velnor_actions_orchestrator::{
    GenerateOptions, OrchestratorError, generate, init_config, merge_internal, plan_internal,
    plan_text, prepare, resolve_root,
};

use crate::args::{Cli, Command};

/// Environment variable selecting the private operation. Never printed.
const OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Environment variable carrying the run key. Never printed.
const RUN_KEY_ENV: &str = "VELNOR_RUN_KEY";
/// Environment variable carrying the runner temp root. Never printed.
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
/// Private plan operation tag.
const PLAN_OP: &str = "plan-v1";
/// Private merge operation tag.
const MERGE_OP: &str = "merge-v1";
/// Private request file name under the run directory.
const REQUEST_FILE: &str = "request.json";
/// Private response file name under the run directory.
const RESPONSE_FILE: &str = "response.json";

/// Private operation selected by the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalOp {
    /// Plan operation.
    Plan,
    /// Merge operation.
    Merge,
}

/// Validated private request: operation plus run directory.
#[derive(Debug)]
struct InternalRequest {
    /// Operation to run.
    op: InternalOp,
    /// Run directory holding the request and response files.
    dir: PathBuf,
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

/// Check the private gate: known op plus a pre-existing request file.
fn gate_request() -> Option<InternalRequest> {
    let op = match env::var(OP_ENV).as_deref() {
        Ok(PLAN_OP) => InternalOp::Plan,
        Ok(MERGE_OP) => InternalOp::Merge,
        _ => return None,
    };
    let runner_temp = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty())?;
    let run_key = env::var(RUN_KEY_ENV)
        .ok()
        .filter(|key| valid_run_key(key))?;
    let dir = Path::new(&runner_temp).join("velnor").join(run_key);
    if !dir.join(REQUEST_FILE).is_file() {
        return None;
    }
    Some(InternalRequest { op, dir })
}

/// Reject empty or path-escaping run keys without printing them.
fn valid_run_key(key: &str) -> bool {
    !key.is_empty() && !key.contains(['/', '\\']) && key != "." && key != ".."
}

/// Read the request file, dispatch to the orchestrator, write the response.
fn run_internal(request: &InternalRequest) -> ExitCode {
    let text = match fs::read_to_string(request.dir.join(REQUEST_FILE)) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let response = match request.op {
        InternalOp::Plan => plan_internal(&text),
        InternalOp::Merge => merge_internal(&text),
    };
    let response = match response {
        Ok(response) => response,
        Err(error) => return fail_internal(&error.to_string()),
    };
    match fs::write(request.dir.join(RESPONSE_FILE), response) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("write response: {error}")),
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
    let report = resolve_root(&cwd).and_then(|root| {
        let preparation = prepare(&root)?;
        generate(&preparation, &options)
    });
    match report {
        Ok(report) => {
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
