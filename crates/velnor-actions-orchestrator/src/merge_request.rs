// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
use crate::request_event::{
    QualificationRunnerContext, qualification_dispatch_for_parts, workflow_event_for,
};

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

fn event_payload_from(value: &std::ffi::OsStr) -> Option<String> {
    crate::safe_read::read_event_file(Path::new(value), crate::safe_read::MAX_REPO_FILE_BYTES).ok()
}

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
    });
    canonical_json_str(&request).map_err(internal_contract)
}

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

pub(crate) fn write_merge_request(request_path: &Path) -> Result<PathBuf, OrchestratorError> {
    let run_key = resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let anchor = Path::new(&temp).to_path_buf();
    let run_dir = anchor.join("velnor").join(&run_key);
    write_merge_request_to(request_path, &run_key, &run_dir, &anchor)
}

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

const MAX_ASSEMBLY_JSON_BYTES: u64 = 4 << 20;

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
