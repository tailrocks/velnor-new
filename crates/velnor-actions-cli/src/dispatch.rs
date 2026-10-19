//! Typed dispatch for public commands and the private file entrypoint.
//!
//! The private gate requires `VELNOR_INTERNAL_OP` plus the exact request file
//! in `VELNOR_REQUEST_FILE`: `write-request-v1` needs GitHub event env and no
//! pre-existing file, `plan-v1`/`merge-v1` need a
//! pre-existing request file, `fetch-reports-v1`/`write-task-report-v1`
//! need runner temp plus the numeric run ID instead, and
//! `write-preseed-manifest-v1` needs runner temp only, and
//! `resolve-qualification-v1` requires a dispatch event, request file, and
//! read-only GitHub token. `resolve-task-execution-v1` requires runner temp
//! and the run ID; it emits only the plan-bound task data frame. Anything else
//! falls through to Clap, so public behavior is byte-identical with or without
//! the environment set.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    ACQUIRE_PREVIEW_LEAVES_OP, ADMIT_HOMEBREW_TAP_OP, ADMIT_PREVIEW_AGGREGATE_OP,
    CAPTURE_REVIEW_COMPLETION_OP, DYNAMIC_MATRIX_OUTPUT_MODE, EMIT_REVIEW_COMPLETION_OP,
    EXECUTE_CHECK_OP, EXECUTE_PREVIEW_PRODUCER_OP, FETCH_OP, MERGE_HOMEBREW_TAP_PR_OP, MERGE_OP,
    OrchestratorError, PLAN_MATRIX_OUTPUT_MODE_ENV, PLAN_OP, PREPARE_HOMEBREW_TAP_OP,
    PRESEED_MANIFEST_OP, PUBLISH_PACKAGE_RELEASE_OP, PlanOutputMode, RECOVER_PACKAGE_RELEASE_OP,
    REPORT_OP, REQUEST_FILE_ENV, SELECT_PREVIEW_TAP_CONTINUATION_OP, SIGN_PACKAGE_RELEASE_OP,
    TASK_EXECUTION_RESOLVER_OP, WRITE_HOMEBREW_TAP_PR_OP, WRITE_REQUEST_OP, execute_check,
    merge_internal, merge_passed, plan_internal, plan_outputs, publish_final_report,
    publish_plan_files, resolve_task_execution, response_path_for, retrieve_reports,
    write_preseed_manifest, write_request, write_task_report,
};

#[path = "dispatch_preview_production.rs"]
mod preview_production;
mod public;
#[path = "dispatch_review_completion.rs"]
mod review_completion;
#[path = "dispatch_tap_continuation.rs"]
mod tap_continuation;
pub(crate) use public::run_public;
#[path = "dispatch_owned_tool_publication.rs"]
mod owned_publication;
#[path = "dispatch_package_release.rs"]
mod package_release;
#[path = "dispatch_package_release_publish.rs"]
mod package_release_publish;
#[path = "dispatch_package_release_recovery.rs"]
mod package_release_recovery;
#[path = "dispatch_tap_merge.rs"]
mod tap_merge;
#[path = "dispatch_tap_prepare.rs"]
mod tap_prepare;

/// Environment variable selecting the private operation. Never printed.
const OP_ENV: &str = "VELNOR_INTERNAL_OP";
/// Environment variable carrying the `$GITHUB_OUTPUT` path. Never printed.
const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
/// Environment variable carrying the runner temp dir. Never printed.
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
/// Environment variable carrying the triggering event name. Never printed.
const GITHUB_EVENT_ENV: &str = "GITHUB_EVENT_NAME";
/// Environment variable carrying the event payload path. Never printed.
const GITHUB_EVENT_PATH_ENV: &str = "GITHUB_EVENT_PATH";

mod operation;
use operation::InternalOp;

/// Validated private request: operation plus exact request-file path.
#[derive(Debug)]
struct InternalRequest {
    /// Operation to run.
    op: InternalOp,
    /// Exact request-file path from the environment.
    path: PathBuf,
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

fn internal_op() -> Option<InternalOp> {
    Some(match env::var(OP_ENV).as_deref() {
        Ok(tag) if tag == SELECT_PREVIEW_TAP_CONTINUATION_OP => InternalOp::TapContinuation,
        Ok(tag) if tag == CAPTURE_REVIEW_COMPLETION_OP => InternalOp::CaptureReviewCompletion,
        Ok(tag) if tag == EMIT_REVIEW_COMPLETION_OP => InternalOp::EmitReviewCompletion,
        Ok(tag) if tag == EXECUTE_CHECK_OP => InternalOp::ExecuteCheck,
        Ok(tag) if tag == ACQUIRE_PREVIEW_LEAVES_OP => InternalOp::AcquirePreviewLeaves,
        Ok(tag) if tag == EXECUTE_PREVIEW_PRODUCER_OP => InternalOp::ExecutePreviewProducer,
        Ok(tag) if tag == ADMIT_PREVIEW_AGGREGATE_OP => InternalOp::AdmitPreviewAggregate,
        Ok(tag) if tag == WRITE_REQUEST_OP => InternalOp::WriteRequest,
        Ok(tag) if tag == PLAN_OP => InternalOp::Plan,
        Ok(tag) if tag == MERGE_OP => InternalOp::Merge,
        Ok(tag) if tag == FETCH_OP => InternalOp::Fetch,
        Ok(tag) if tag == REPORT_OP => InternalOp::Report,
        Ok(tag) if tag == PRESEED_MANIFEST_OP => InternalOp::PreseedManifest,
        Ok("repo-policy-v1") => InternalOp::RepoPolicy,
        Ok(tag) if crate::dispatch_qualification::is_resolver_op(tag) => {
            InternalOp::ResolveQualification
        }
        Ok(tag) if tag == TASK_EXECUTION_RESOLVER_OP => InternalOp::ResolveTaskExecution,
        Ok(tag) if tag == SIGN_PACKAGE_RELEASE_OP => InternalOp::PackageSigner,
        Ok(tag) if tag == PUBLISH_PACKAGE_RELEASE_OP => InternalOp::PackagePublisher,
        Ok(tag) if tag == RECOVER_PACKAGE_RELEASE_OP => InternalOp::PackageRecovery,
        Ok(tag) if tag == PREPARE_HOMEBREW_TAP_OP => InternalOp::TapPrepare,
        Ok(tag) if tag == ADMIT_HOMEBREW_TAP_OP => InternalOp::TapAdmission,
        Ok(tag) if tag == WRITE_HOMEBREW_TAP_PR_OP => InternalOp::TapWriter,
        Ok(tag) if tag == MERGE_HOMEBREW_TAP_PR_OP => InternalOp::TapMerge,
        _ => return None,
    })
}

/// Check the private gate: known op plus request-file presence by op.
///
/// Fetch and report take no request file: they need the runner-temp
/// velnor directory plus the numeric run ID instead. The preseed-manifest op
/// takes no request file either: runner temp scopes its output.
fn gate_request() -> Option<InternalRequest> {
    let op = internal_op()?;
    if op == InternalOp::ResolveTaskExecution {
        if !env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty()) {
            return None;
        }
        return runner_velnor_dir().map(|path| InternalRequest { op, path });
    }
    if op == InternalOp::Fetch || op == InternalOp::Report || op == InternalOp::ExecuteCheck {
        if env::var("GITHUB_RUN_ID").is_ok_and(|id| !id.is_empty()) {
            return runner_velnor_dir().map(|path| InternalRequest { op, path });
        }
        return None;
    }
    if op == InternalOp::PreseedManifest {
        return runner_velnor_dir().map(|path| InternalRequest { op, path });
    }
    if op == InternalOp::RepoPolicy {
        let root = crate::dispatch_repo_policy::gate_root()?;
        return Some(InternalRequest { op, path: root });
    }
    let path = env::var_os(REQUEST_FILE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    match op {
        InternalOp::TapContinuation => {
            return tap_continuation::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::CaptureReviewCompletion | InternalOp::EmitReviewCompletion => {
            return review_completion::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::WriteRequest => {
            if !request_can_be_materialized(&path) {
                return None;
            }
        }
        InternalOp::Plan | InternalOp::Merge | InternalOp::ExecutePreviewProducer => {
            if !path.is_file() {
                return None;
            }
        }
        InternalOp::ResolveQualification => {
            if !crate::dispatch_qualification::request_is_eligible(&path) {
                return None;
            }
        }
        InternalOp::PackageSigner
        | InternalOp::PackagePublisher
        | InternalOp::PackageRecovery
        | InternalOp::AdmitPreviewAggregate
        | InternalOp::AcquirePreviewLeaves => {
            return package_release::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::TapAdmission => {
            return tap_prepare::gate_admission_request(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::TapPrepare => {
            return tap_prepare::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::TapWriter => {
            return tap_prepare::gate_writer_request(&path)
                .map(|path| InternalRequest { op, path });
        }
        InternalOp::TapMerge => {
            return tap_merge::gate_request(&path).map(|path| InternalRequest { op, path });
        }
        InternalOp::Fetch
        | InternalOp::Report
        | InternalOp::PreseedManifest
        | InternalOp::RepoPolicy
        | InternalOp::ExecuteCheck
        | InternalOp::ResolveTaskExecution => {}
    }
    Some(InternalRequest { op, path })
}

fn request_can_be_materialized(path: &Path) -> bool {
    !path.exists()
        && env::var(GITHUB_EVENT_ENV).is_ok_and(|name| !name.is_empty())
        && env::var_os(GITHUB_EVENT_PATH_ENV).is_some_and(|value| !value.is_empty())
}

/// Runner-temp velnor directory for file-less private operations.
///
/// `None` when `RUNNER_TEMP` is unset or empty; shared by the fetch,
/// report, and preseed-manifest gate branches so the scoping rule has
/// one definition.
fn runner_velnor_dir() -> Option<PathBuf> {
    env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .map(|temp| Path::new(&temp).join("velnor"))
}

/// Run one validated private operation.
fn run_internal(request: &InternalRequest) -> ExitCode {
    match request.op {
        InternalOp::ExecuteCheck => match execute_check() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::ExecutePreviewProducer => preview_production::produce(),
        InternalOp::AcquirePreviewLeaves => preview_production::acquire(),
        InternalOp::TapContinuation => tap_continuation::run(),
        InternalOp::CaptureReviewCompletion => review_completion::capture(),
        InternalOp::EmitReviewCompletion => review_completion::emit(),
        InternalOp::AdmitPreviewAggregate => preview_production::admit(),
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
        InternalOp::Report => match write_task_report() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::PreseedManifest => match write_preseed_manifest() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::RepoPolicy => crate::dispatch_repo_policy::run(&request.path),
        InternalOp::ResolveQualification => crate::dispatch_qualification::run(&request.path),
        InternalOp::ResolveTaskExecution => match resolve_task_execution() {
            Ok(frame) => match std::io::stdout().lock().write_all(&frame) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => fail_internal(&format!("write task execution frame: {error}")),
            },
            Err(error) => fail_internal(&error.to_string()),
        },
        InternalOp::PackageSigner => package_release::run(),
        InternalOp::PackagePublisher => package_release_publish::run(),
        InternalOp::PackageRecovery => package_release_recovery::run(),
        InternalOp::TapPrepare => tap_prepare::run(),
        InternalOp::TapAdmission => tap_prepare::run_admission(),
        InternalOp::TapWriter => tap_prepare::run_writer(),
        InternalOp::TapMerge => tap_merge::run(),
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
pub(crate) fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
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

/// Collapse error values to one log line so untrusted input cannot inject
/// additional stderr records.
fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
