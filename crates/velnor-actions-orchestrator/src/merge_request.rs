//! Event-time `merge-v1` request assembly from downloaded artifacts.
//!
//! The final job downloads the plan artifact (`plan.json`, `matrix.json`)
//! plus every matrix-report artifact under `reports/<artifact-id>/` before
//! the merge step; this module assembles those files into the canonical
//! merge-request JSON that [`crate::merge_internal`] consumes. Candidate
//! qualification evidence is the candidate job's `needs` conclusion,
//! not a separate report file: no producer ever wrote one, so requiring
//! it failed candidate mode closed on every run.
//!
//! Required validator conclusions arrive through the `VELNOR_NEEDS_JSON`
//! channel: a JSON object mapping each job in the final gate's `needs`
//! to its conclusion, either directly (`{"plan": "success"}`) or in
//! `toJSON(needs)` shape (`{"plan": {"result": "success"}}`). The
//! renderer also emits the static expected inventory through
//! `VELNOR_NEEDS_EXPECTED`; assembly declares it as the required
//! inventory (minus the matrix-driver job, whose legs prove themselves
//! through per-leg reports) and fails closed on a missing or unparsable
//! channel or any observed-vs-expected divergence.
//!
//! Assembly never drops evidence silently: every missing or unparsable
//! artifact becomes an explicit `assembly_errors` entry that fails the
//! verdict, so the merge still emits its diagnostic `planning_failed`
//! report instead of dying in request assembly.

// Needs-channel parsing lives beside assembly so `lib.rs` stays untouched.
#[path = "needs_channel.rs"]
mod needs_channel;

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical_json_str;

use self::needs_channel::{NEEDS_ENV, NEEDS_EXPECTED_ENV, parse_needs};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;

/// Assemble one canonical merge request from a run directory.
///
/// Reads `plan.json`, `matrix.json`, exactly the plan-expected
/// `reports/<artifact-id>/matrix-report.json` files plus their
/// `tasks/<task-report-id>.json` files (sorted by ID for determinism;
/// anything else under `reports/` is ignored, never globbed), plus
/// optional `baseline.json`.
/// Validator inventory and conclusions come from `VELNOR_NEEDS_JSON`. Every
/// missing or unparsable input is recorded in `assembly_errors`, never
/// dropped, so the merge judges the gap explicitly.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
pub fn assemble_merge_request(run_key: &str, run_dir: &Path) -> Result<String, OrchestratorError> {
    let needs = std::env::var(NEEDS_ENV).ok();
    let expected = std::env::var(NEEDS_EXPECTED_ENV).ok();
    assemble_with_needs(run_key, run_dir, needs.as_deref(), expected.as_deref())
}

/// Assemble one merge request with explicit needs channels.
///
/// The public wrapper reads `VELNOR_NEEDS_JSON` plus
/// `VELNOR_NEEDS_EXPECTED`; tests pass the channels explicitly for
/// determinism.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
pub(crate) fn assemble_with_needs(
    run_key: &str,
    run_dir: &Path,
    needs: Option<&str>,
    expected: Option<&str>,
) -> Result<String, OrchestratorError> {
    let mut errors = Vec::new();
    let plan = read_json(run_dir, "plan.json", "plan", true, &mut errors);
    let matrix = read_json(run_dir, "matrix.json", "matrix", true, &mut errors);
    let (reports, task_reports) = crate::retrieve_reports::read_staged_reports(
        run_key,
        &plan,
        &run_dir.join("reports"),
        &mut errors,
    );
    let baseline = read_json(run_dir, "baseline.json", "baseline", false, &mut errors);
    let (inventory, jobs) = parse_needs(needs, expected, &mut errors);
    let request = serde_json::json!({
        "schema": 1,
        "run_key": run_key,
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
        "task_reports": task_reports,
        "required_job_ids": inventory,
        "required_jobs": jobs,
        "assembly_errors": errors,
        "baseline_manifest": baseline,
    });
    canonical_json_str(&request).map_err(internal_contract)
}

/// Materialize the merge request from the environment and run directory.
///
/// Resolves the run key from `GITHUB_RUN_ID`/`GITHUB_RUN_ATTEMPT` and the
/// run directory from `RUNNER_TEMP`, then delegates to
/// [`write_merge_request_to`].
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env or unwritable
/// paths; [`OrchestratorError::Io`] for unreadable artifact JSON.
pub(crate) fn write_merge_request(request_path: &Path) -> Result<PathBuf, OrchestratorError> {
    let run_key = resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&temp).join("velnor").join(&run_key);
    write_merge_request_to(request_path, &run_key, &run_dir)
}

/// Assemble and exclusively write one merge request file.
///
/// The file is written exclusively (a pre-existing file errors, never
/// overwritten), matching the plan request writer. Missing inputs are
/// recorded in the request, not refused here.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for a pre-existing request
/// file or unwritable paths.
pub(crate) fn write_merge_request_to(
    request_path: &Path,
    run_key: &str,
    run_dir: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let request = assemble_merge_request(run_key, run_dir)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| OrchestratorError::io(parent.display().to_string(), err.to_string()))?;
    }
    crate::exclusive_write::write_exclusive(&path, request.as_bytes(), "request")?;
    Ok(path)
}

/// Maximum bytes read from one assembled JSON artifact.
///
/// Plans carry obligations plus the matrix; four megabytes fails closed
/// on runaway payloads without truncating legitimate evidence.
const MAX_ASSEMBLY_JSON_BYTES: u64 = 4 << 20;

/// Read one JSON artifact; failures become null plus an explicit error.
///
/// Absence errors only for required artifacts; corruption, symlinks,
/// unreadable paths, and oversize payloads always error. Reads enforce
/// the shared staged-text gates (symlink rejection plus size bound),
/// so `baseline.json` and every other assembly input harden alike.
fn read_json(
    run_dir: &Path,
    name: &str,
    kind: &str,
    required: bool,
    errors: &mut Vec<String>,
) -> serde_json::Value {
    match crate::retrieve_reports::read_staged_text(&run_dir.join(name), MAX_ASSEMBLY_JSON_BYTES) {
        Ok(text) => {
            if let Ok(value) = serde_json::from_str(&text) {
                value
            } else {
                errors.push(format!("unparsable_{kind}"));
                serde_json::Value::Null
            }
        }
        Err("missing") => {
            if required {
                errors.push(format!("missing_{kind}"));
            }
            serde_json::Value::Null
        }
        Err("symlink") => {
            errors.push(format!("symlink_{kind}"));
            serde_json::Value::Null
        }
        Err("oversize") => {
            errors.push(format!("oversize_{kind}"));
            serde_json::Value::Null
        }
        Err(_) => {
            errors.push(format!("unreadable_{kind}"));
            serde_json::Value::Null
        }
    }
}

#[cfg(test)]
#[path = "merge_request_tests.rs"]
mod merge_request_tests;
