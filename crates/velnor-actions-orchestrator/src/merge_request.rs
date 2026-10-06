//! Event-time `merge-v1` request assembly from downloaded artifacts.
//!
//! The final job downloads the plan artifact (`plan.json`, `matrix.json`)
//! plus every matrix-report artifact under `reports/<artifact-id>/` before
//! the merge step; this module assembles those files into the canonical
//! merge-request JSON that [`crate::merge_internal`] consumes. Candidate
//! qualification evidence is the candidate job's `needs` conclusion,
//! not a separate report file: no producer ever wrote one, so requiring
//! it failed candidate mode closed on every run. The conclusion is
//! bound to the plan head by the candidate attestation (S3), which the
//! merge re-checks; the manifest's remaining fields stay consumer-side
//! audit data (S10/D6).
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

use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    NEEDS_EXPECTED_ENV, QualificationDispatch, canonical_json_str, parse_strict_json,
};

use self::needs_channel::{NEEDS_ENV, parse_needs};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_request::resolve_run_key;
use crate::merge::required_evidence::MAX_BASELINE_MANIFEST_BYTES;
use crate::request_event::{
    QualificationRunnerContext, qualification_dispatch_for_parts, workflow_event_for,
};

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
    let event_name = std::env::var("GITHUB_EVENT_NAME").ok();
    let event_payload = std::env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .as_deref()
        .and_then(event_payload_from);
    assemble_with_needs(
        run_key,
        run_dir,
        needs.as_deref(),
        expected.as_deref(),
        event_name.as_deref(),
        event_payload.as_deref(),
    )
}

/// Event payload through the bounded, symlink-rejecting reader.
///
/// Unreadable payloads become `None` so assembly records the gap
/// explicitly instead of failing the whole request; the plan side
/// reads the same file through the same helper.
fn event_payload_from(value: &std::ffi::OsStr) -> Option<String> {
    crate::safe_read::read_event_file(Path::new(value), crate::safe_read::MAX_REPO_FILE_BYTES).ok()
}

/// Assemble one merge request with explicit needs and event channels.
///
/// The public wrapper reads `VELNOR_NEEDS_JSON` plus
/// `VELNOR_NEEDS_EXPECTED` plus the GitHub event name/payload; tests
/// pass every channel explicitly for determinism.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
pub(crate) fn assemble_with_needs(
    run_key: &str,
    run_dir: &Path,
    needs: Option<&str>,
    expected: Option<&str>,
    event_name: Option<&str>,
    event_payload: Option<&str>,
) -> Result<String, OrchestratorError> {
    let mut errors = Vec::new();
    let actual_event = resolve_actual_event(event_name, event_payload, &mut errors);
    let actual_qualification = resolve_actual_qualification(event_name, event_payload, &mut errors);
    let plan = read_json(run_dir, "plan.json", "plan", true, &mut errors);
    let matrix = read_json(run_dir, "matrix.json", "matrix", true, &mut errors);
    let (reports, task_reports) = crate::retrieve_reports::read_staged_reports(
        run_key,
        &plan,
        &run_dir.join("reports"),
        &mut errors,
    );
    let check_proofs = crate::check_evidence::gate::read_proofs(
        &plan,
        &run_dir.join("reports"),
        &task_reports,
        &mut errors,
    );
    let baseline = read_json_with_limit(
        run_dir,
        "baseline.json",
        "baseline",
        false,
        u64::try_from(MAX_BASELINE_MANIFEST_BYTES).unwrap_or(u64::MAX),
        &mut errors,
    );
    let (inventory, jobs) = parse_needs(needs, expected, &mut errors);
    let attestation = read_attestation(run_dir, &inventory, &mut errors);
    let request = serde_json::json!({
        "schema": 1,
        "run_key": run_key,
        "actual_event": actual_event,
        "actual_qualification": actual_qualification,
        "candidate_attestation": attestation,
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
        "task_reports": task_reports,
        "check_proofs": check_proofs,
        "required_job_ids": inventory,
        "required_jobs": jobs,
        "assembly_errors": errors,
        "baseline_manifest": baseline,
    });
    canonical_json_str(&request).map_err(internal_contract)
}

/// Head-bound candidate attestation, required in candidate mode only.
///
/// Candidate mode is the candidate job's presence in the required
/// inventory. Outside it the file is never read (no error); inside it
/// a missing or unreadable attestation records an explicit error and
/// the merge fails closed, never silent.
fn read_attestation(
    run_dir: &Path,
    inventory: &[String],
    errors: &mut Vec<String>,
) -> serde_json::Value {
    use velnor_actions_contract::{CANDIDATE_ATTESTATION_FILENAME, CANDIDATE_EVIDENCE_SUBDIR};
    use velnor_actions_workflow_renderer::render::CANDIDATE_JOB_ID;
    if !inventory.iter().any(|id| id == CANDIDATE_JOB_ID) {
        return serde_json::Value::Null;
    }
    let relpath = format!("{CANDIDATE_EVIDENCE_SUBDIR}/{CANDIDATE_ATTESTATION_FILENAME}");
    read_json(run_dir, &relpath, "candidate_attestation", true, errors)
}

/// Merge-time triggering event from explicit GitHub parts.
///
/// The merge job runs in the same run as the plan job, so its
/// `GITHUB_EVENT_NAME` is the ground truth the plan's stamped event
/// must match; a forged plan artifact claiming a stronger event (push
/// trust on PR content) fails closed at merge instead of inheriting
/// its own stamp. Resolution shares [`workflow_event_for`] with plan
/// requests so fork handling can never disagree. Only `pull_request`
/// requires the payload (fork detection); every other event resolves
/// from the name alone, but a present-but-malformed payload still
/// fails closed (a corrupt runner channel proves nothing).
fn resolve_actual_event(
    event_name: Option<&str>,
    event_payload: Option<&str>,
    errors: &mut Vec<String>,
) -> Option<velnor_actions_contract::WorkflowEvent> {
    let Some(name) = event_name.filter(|name| !name.trim().is_empty()) else {
        errors.push("missing_actual_event".to_owned());
        return None;
    };
    let payload = match event_payload {
        Some(text) => {
            if let Ok(payload) = parse_strict_json(text) {
                payload
            } else {
                errors.push("malformed_actual_payload".to_owned());
                return None;
            }
        }
        None if matches!(name, "pull_request" | "workflow_dispatch") => {
            errors.push("missing_actual_payload".to_owned());
            return None;
        }
        None => serde_json::Value::Null,
    };
    match workflow_event_for(name, &payload) {
        Ok(event) => Some(event),
        Err(OrchestratorError::Internal { problem }) => {
            errors.push(problem);
            None
        }
        Err(_) => {
            errors.push("actual_event_error".to_owned());
            None
        }
    }
}

/// Reacquire qualification provenance from this merge runner's actual
/// dispatch payload and immutable GitHub environment.
fn resolve_actual_qualification(
    event_name: Option<&str>,
    event_payload: Option<&str>,
    errors: &mut Vec<String>,
) -> Option<QualificationDispatch> {
    let name = event_name?;
    if name != "workflow_dispatch" {
        return None;
    }
    let Some(text) = event_payload else {
        errors.push("missing_actual_payload".to_owned());
        return None;
    };
    let Ok(payload) = parse_strict_json(text) else {
        errors.push("malformed_actual_payload".to_owned());
        return None;
    };
    let read = |name: &str| std::env::var(name).ok();
    match qualification_dispatch_for_parts(
        name,
        &payload,
        QualificationRunnerContext {
            repository: read("GITHUB_REPOSITORY").as_deref(),
            git_ref: read("GITHUB_REF").as_deref(),
            ref_protected: read("GITHUB_REF_PROTECTED").as_deref(),
            workflow_ref: read("GITHUB_WORKFLOW_REF").as_deref(),
            workflow_sha: read("GITHUB_WORKFLOW_SHA").as_deref(),
            source_sha: read("GITHUB_SHA").as_deref(),
            run_id: read("GITHUB_RUN_ID").as_deref(),
            run_attempt: read("GITHUB_RUN_ATTEMPT").as_deref(),
        },
    ) {
        Ok(context) => context,
        Err(OrchestratorError::Internal { problem }) => {
            errors.push(problem);
            None
        }
        Err(_) => {
            errors.push("actual_qualification_error".to_owned());
            None
        }
    }
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
    let anchor = Path::new(&temp).to_path_buf();
    let run_dir = anchor.join("velnor").join(&run_key);
    write_merge_request_to(request_path, &run_key, &run_dir, &anchor)
}

/// Assemble and exclusively write one merge request file.
///
/// The file is written exclusively (a pre-existing file errors, never
/// overwritten), matching the plan request writer. Missing inputs are
/// recorded in the request, not refused here. Parents are created
/// under `anchor` with symlink refusal, like the plan request writer.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for a pre-existing request
/// file, anchor escapes, or unwritable paths.
pub(crate) fn write_merge_request_to(
    request_path: &Path,
    run_key: &str,
    run_dir: &Path,
    anchor: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let request = assemble_merge_request(run_key, run_dir)?;
    if let Some(parent) = path.parent() {
        crate::exclusive_write::create_dir_no_symlink(anchor, parent)?;
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
/// Absence errors only for required artifacts; corruption, duplicate
/// keys, symlinks, unreadable paths, and oversize payloads always
/// error. Reads enforce the shared staged-text gates (symlink rejection
/// plus size bound) and the strict key check, so `baseline.json` and
/// every other assembly input hardens alike.
fn read_json(
    run_dir: &Path,
    name: &str,
    kind: &str,
    required: bool,
    errors: &mut Vec<String>,
) -> serde_json::Value {
    read_json_with_limit(
        run_dir,
        name,
        kind,
        required,
        MAX_ASSEMBLY_JSON_BYTES,
        errors,
    )
}

/// Read one JSON artifact with its own byte contract.
fn read_json_with_limit(
    run_dir: &Path,
    name: &str,
    kind: &str,
    required: bool,
    max_bytes: u64,
    errors: &mut Vec<String>,
) -> serde_json::Value {
    match crate::retrieve_reports::read_staged_text(&run_dir.join(name), max_bytes) {
        Ok(text) => {
            if let Ok(value) = parse_strict_json(&text) {
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
#[path = "merge_event_tests.rs"]
mod merge_event_tests;
#[cfg(test)]
#[path = "merge_request_tests.rs"]
mod merge_request_tests;

#[cfg(test)]
#[path = "actual_event_strict_tests.rs"]
mod actual_event_strict_tests;
