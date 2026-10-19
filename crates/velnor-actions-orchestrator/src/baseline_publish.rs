//! Publish baseline evidence from successful reports in the current run.
//!
//! The operation checks the push request, plan, matrix entries, and task
//! reports. It writes one proof for each obligation after all its matrix
//! reports pass. It does not use baseline evidence to skip verification.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    ManifestTaskProof, ObligationDecision, TaskReport, TaskStatus, Trust, WorkflowEvent,
    canonical_json_bytes, digest_b3, parse_strict_json,
};

use crate::OrchestratorError;
use crate::internal::{SCHEMA, check_schema, internal, internal_contract};
use crate::internal_request::resolve_run_key;
use crate::request_event::{request_refs, workflow_event_for};

/// Schema-2 publication task row. Coverage lineage is intentionally absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedTask {
    task_id: String,
    task_digest: String,
    input_digest: String,
    closure_digest: String,
    proof_run_id: u64,
    observed_run_id: u64,
    carried_from: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_data: Option<serde_json::Value>,
    proof: Option<ManifestTaskProof>,
}

/// Existing schema-2 baseline artifact, written only from this run's successful reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineManifest {
    schema: u32,
    repository_id: String,
    source_commit: String,
    #[serde(rename = "ref")]
    ref_: String,
    event: String,
    workflow_ref: String,
    run_id: u64,
    run_attempt: u64,
    final_status: String,
    generator_version: String,
    generator_sha256: String,
    compatibility_id: String,
    artifact_id: u64,
    artifact_name: String,
    tasks: Vec<PublishedTask>,
    parent: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at_unix: Option<u64>,
}

/// Publish operation tag.
pub const PUBLISH_OP: &str = "publish-baseline-v1";
/// Staged baseline filename inside the run directory.
pub(crate) const BASELINE_FILENAME: &str = "baseline.json";
/// Separate output directory so publication never overwrites its parent.
pub(crate) const PUBLISHED_BASELINE_DIR: &str = "published-baseline";

/// `publish-baseline-v1` request: push refs plus protected-branch evidence.
///
/// Materialized at the request boundary from the GitHub environment;
/// the op consumes this file and reads no `GITHUB_*` ambient state
/// itself. Unknown fields (including caller-supplied manifest
/// metadata) reject via `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Consuming operation; must be `publish-baseline-v1` when present.
    #[serde(default)]
    op: Option<String>,
    /// Triggering event; must be `push`.
    event: WorkflowEvent,
    /// Push event's `before` commit, null only for a branch creation.
    #[serde(default)]
    base: Option<String>,
    /// Pushed head commit.
    head: String,
    /// Runner-owned repository slug (`owner/repo`) for provenance.
    #[serde(default)]
    repository: Option<String>,
    /// Pushed ref from the event payload.
    #[serde(default)]
    git_ref: Option<String>,
    /// Repository default branch from the event payload.
    #[serde(default)]
    default_branch: Option<String>,
}

/// Publish outputs: the derived artifact name for the upload step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOutputs {
    /// Derived `velnor-baseline-<commit>-<compat>` artifact name.
    pub artifact_name: String,
}

/// Materialize one canonical publish request from explicit inputs.
///
/// Parses the event payload exactly like plan requests (shared event
/// and ref resolution, so the layers can never disagree), records the
/// pushed ref plus the payload's default branch, and writes the file
/// exclusively under `anchor`. The op enforces the push gate; this
/// writer records, never judges.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed payloads,
/// unsupported events, missing refs, anchor escapes, or unwritable
/// paths.
pub(crate) fn write_publish_request(
    request_path: &Path,
    event_name: &str,
    payload_json: &str,
    github_sha: Option<&str>,
    repository: Option<&str>,
    anchor: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let payload: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|_| internal("malformed_event_payload"))?;
    let event = workflow_event_for(event_name, &payload)?;
    let (base, head) = request_refs(event, &payload, github_sha)?;
    let request = serde_json::json!({
        "schema": SCHEMA,
        "op": PUBLISH_OP,
        "event": event,
        "base": base,
        "head": head,
        "repository": repository.filter(|slug| !slug.is_empty()),
        "git_ref": nonempty(payload.get("ref").and_then(serde_json::Value::as_str)),
        "default_branch": nonempty(
            payload
                .get("repository")
                .and_then(|repo| repo.get("default_branch"))
                .and_then(serde_json::Value::as_str)
        ),
    });
    let bytes = canonical_json_bytes(&request).map_err(internal_contract)?;
    if let Some(parent) = path.parent() {
        crate::exclusive_write::create_dir_no_symlink(anchor, parent)?;
    }
    crate::exclusive_write::write_exclusive(&path, &bytes, "request")?;
    Ok(path)
}

/// Trimmed non-empty string, if any.
fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Build and stage the trusted `baseline.json` for one push.
///
/// Binds the downloaded plan to the request refs, derives the exact
/// artifact name the lookup reproduces, self-checks the manifest
/// through consumer validation, and stages it under the run
/// directory for the upload step. Returns the derived artifact name.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests,
/// refused gates, unreadable plans, and unwritable paths;
/// [`OrchestratorError::Io`] for IO failures.
pub fn baseline_publish(
    request_json: &str,
    runner_temp: &Path,
) -> Result<PublishOutputs, OrchestratorError> {
    let run_key = resolve_run_key(None)?;
    baseline_publish_to(request_json, &run_key, runner_temp)
}

/// Build and stage with an explicit run key (testable core).
///
/// The public wrapper resolves the key from the GitHub environment;
/// tests pass it explicitly for hermetic runs.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests,
/// refused gates, unreadable plans, and unwritable paths;
/// [`OrchestratorError::Io`] for IO failures.
pub(crate) fn baseline_publish_to(
    request_json: &str,
    run_key: &str,
    runner_temp: &Path,
) -> Result<PublishOutputs, OrchestratorError> {
    velnor_actions_contract::validate_run_key(run_key).map_err(internal_contract)?;
    let request = publish_request(request_json)?;
    let (run_id, run_attempt) = ci_run_ids(run_key)?;
    publish_gate(&request)?;
    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    bind_plan(&request, &plan)?;
    let run_dir = runner_temp.join("velnor").join(run_key);
    let downloaded = crate::retrieve_reports::retrieve_reports_to(run_id, &run_dir);
    if downloaded == 0 {
        return Err(internal("publish_refused:reports_missing"));
    }
    let plan_value =
        serde_json::to_value(&plan).map_err(|_| internal("publish_refused:plan_encode"))?;
    let mut report_errors = Vec::new();
    let (_, task_values) = crate::retrieve_reports::read_staged_reports(
        run_key,
        &plan_value,
        &run_dir.join("reports"),
        &mut report_errors,
    );
    if !report_errors.is_empty() {
        return Err(internal("publish_refused:reports_invalid"));
    }
    let reports = task_values
        .into_iter()
        .map(serde_json::from_value::<TaskReport>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| internal("publish_refused:reports_invalid"))?;
    let manifest = publish_manifest(&request, &plan, run_id, run_attempt, &reports)?;
    self_check(&manifest)?;
    let bytes = canonical_json_bytes(&manifest).map_err(internal_contract)?;
    let output_dir = runner_temp
        .join("velnor")
        .join(run_key)
        .join(PUBLISHED_BASELINE_DIR);
    crate::exclusive_write::create_dir_no_symlink(runner_temp, &output_dir)?;
    let path = output_dir.join(BASELINE_FILENAME);
    crate::exclusive_write::write_exclusive(&path, &bytes, "baseline")?;
    Ok(PublishOutputs {
        artifact_name: manifest.artifact_name,
    })
}

/// Parse and validate one publish request envelope.
fn publish_request(request_json: &str) -> Result<PublishRequest, OrchestratorError> {
    let envelope = parse_strict_json(request_json).map_err(internal_contract)?;
    let request: PublishRequest =
        serde_json::from_value(envelope).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    if request.op.as_deref().is_some_and(|op| op != PUBLISH_OP) {
        return Err(internal("op_mismatch"));
    }
    Ok(request)
}

/// Numeric run IDs from one CI run key; local runs refuse to publish.
fn ci_run_ids(run_key: &str) -> Result<(u64, u64), OrchestratorError> {
    if run_key == "local" {
        return Err(internal("publish_refused:local_run"));
    }
    run_key
        .strip_prefix('r')
        .and_then(|rest| rest.split_once("-a"))
        .and_then(|(id, attempt)| Some((id.parse::<u64>().ok()?, attempt.parse::<u64>().ok()?)))
        .ok_or_else(|| internal("bad_run_key"))
}

/// Refuse anything but a protected default-branch push.
///
/// The event must be `push`, the pushed ref must equal the payload's
/// own default branch, and the repository slug must be present and
/// well-formed. Non-push runs never reach here (the job gates them),
/// but a hand-edited workflow must still fail closed.
fn publish_gate(request: &PublishRequest) -> Result<(), OrchestratorError> {
    if request.event != WorkflowEvent::Push {
        return Err(internal("publish_refused:wrong_event"));
    }
    let protected = request
        .default_branch
        .as_deref()
        .filter(|branch| velnor_actions_contract::is_valid_branch_name(branch))
        .map(|branch| format!("refs/heads/{branch}"));
    if protected.is_none() || request.git_ref.as_ref() != protected.as_ref() {
        return Err(internal("publish_refused:unprotected_ref"));
    }
    let anchored = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug);
    if anchored.is_none() {
        return Err(internal("publish_refused:repository_unanchored"));
    }
    Ok(())
}

/// Bind the downloaded plan to the pushed head and event.
///
/// The plan head must equal the pushed commit and the plan event must
/// be the push itself; anything else proves a stale or forged plan
/// artifact, never this push.
fn bind_plan(
    request: &PublishRequest,
    plan: &velnor_actions_contract::Plan,
) -> Result<(), OrchestratorError> {
    if plan.base != request.base {
        return Err(internal("publish_refused:base_mismatch"));
    }
    if plan.head != request.head {
        return Err(internal("publish_refused:head_mismatch"));
    }
    if plan.event != WorkflowEvent::Push {
        return Err(internal("publish_refused:plan_event_mismatch"));
    }
    Ok(())
}

/// Build the trusted manifest over the plan's executed obligations.
///
/// Each executed obligation may have multiple matrix-lane reports. Validate
/// every planned lane independently, then emit one manifest proof per task.
fn publish_manifest(
    request: &PublishRequest,
    plan: &velnor_actions_contract::Plan,
    run_id: u64,
    run_attempt: u64,
    reports: &[TaskReport],
) -> Result<BaselineManifest, OrchestratorError> {
    if plan.generator.sha256.is_empty() || plan.generator.sha256.chars().any(char::is_uppercase) {
        return Err(internal("publish_refused:generator_unverifiable"));
    }
    if reports.len() != plan.matrix.include.len() {
        return Err(internal("publish_refused:task_report_inventory_mismatch"));
    }
    let mut tasks = Vec::new();
    for obligation in &plan.obligations {
        if obligation.decision != ObligationDecision::Execute {
            return Err(internal("publish_refused:nonexecuted_obligation"));
        }
        let entries: Vec<_> = plan
            .matrix
            .include
            .iter()
            .filter(|entry| entry.task_id == obligation.task_id)
            .collect();
        if entries.is_empty() {
            return Err(internal("publish_refused:task_matrix_missing"));
        }
        let mut matched = Vec::with_capacity(entries.len());
        for entry in &entries {
            let mut lane_reports = reports.iter().filter(|report| {
                report.task_id == obligation.task_id
                    && report.matrix_id == entry.id
                    && report.matrix_key == entry.matrix_key
            });
            let report = lane_reports
                .next()
                .ok_or_else(|| internal("publish_refused:task_report_missing"))?;
            if lane_reports.next().is_some() {
                return Err(internal("publish_refused:duplicate_matrix_report"));
            }
            if report.task_digest != obligation.task_digest
                || report.event != WorkflowEvent::Push
                || report.run_key != plan.run_key
                || report.trust != Trust::Trusted
                || report.status != TaskStatus::Executed
                || report.exit_code != 0
            {
                return Err(internal("publish_refused:task_report_mismatch"));
            }
            if entry.task_id != obligation.task_id {
                return Err(internal("publish_refused:matrix_task_mismatch"));
            }
            report
                .validate_for_plan_entry(entry)
                .map_err(|_| internal("publish_refused:task_report_invalid"))?;
            matched.push((entry, report));
        }
        if reports.iter().any(|report| {
            report.task_id == obligation.task_id
                && !matched.iter().any(|(entry, _)| {
                    report.matrix_id == entry.id && report.matrix_key == entry.matrix_key
                })
        }) {
            return Err(internal("publish_refused:matrix_report_mismatch"));
        }
        let (entry, _) = matched
            .iter()
            .find(|(entry, _)| entry.job_id == obligation.job_id)
            .ok_or_else(|| internal("publish_refused:primary_matrix_lane_missing"))?;
        let toolchain_id = entry
            .toolchain_id
            .as_deref()
            .ok_or_else(|| internal("publish_refused:toolchain_identity_missing"))?;
        let proof = ManifestTaskProof::new(
            &obligation.task_id,
            &obligation.task_digest,
            &obligation.input_digest,
            &obligation.graph_digest,
            toolchain_id,
            &obligation.mbx_digest,
            &entry.planned_platform.platform_id,
            &obligation.profile,
            run_id,
        )
        .map_err(internal_contract)?;
        tasks.push(PublishedTask {
            task_id: obligation.task_id.clone(),
            task_digest: obligation.task_digest.clone(),
            input_digest: obligation.input_digest.clone(),
            closure_digest: obligation.closure_digest.clone(),
            proof_run_id: run_id,
            observed_run_id: run_id,
            carried_from: None,
            external_data: None,
            proof: Some(proof),
        });
    }
    tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let compat = baseline_compat_for_plan(plan)?;
    let name = velnor_actions_contract::artifact_id_for_baseline(&request.head, &compat)
        .map_err(|_| internal("publish_refused:bad_source_commit"))?;
    let slug = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug)
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let git_ref = request
        .git_ref
        .clone()
        .ok_or_else(|| internal("publish_refused:unprotected_ref"))?;
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    Ok(BaselineManifest {
        schema: velnor_actions_contract::Plan::SCHEMA,
        repository_id: digest_b3(format!("github.com/{slug}").as_bytes()),
        source_commit: request.head.clone(),
        ref_: git_ref.clone(),
        event: "push".to_owned(),
        workflow_ref: format!("{slug}/{workflow}@{git_ref}"),
        run_id,
        run_attempt,
        final_status: "passed".to_owned(),
        generator_version: plan.generator.version.clone(),
        generator_sha256: plan.generator.sha256.clone(),
        compatibility_id: compat,
        artifact_id: baseline_artifact_numeric_id(&name),
        artifact_name: name,
        tasks,
        parent: None,
        expires_at_unix: None,
    })
}

/// Check publication schema and each task proof before serialization.
fn self_check(manifest: &BaselineManifest) -> Result<(), OrchestratorError> {
    if manifest.schema != 2
        || manifest.event != "push"
        || manifest.final_status != "passed"
        || manifest.tasks.is_empty()
        || manifest.tasks.iter().any(|task| task.proof.is_none())
    {
        return Err(internal("publish_self_check:invalid_manifest"));
    }
    for task in &manifest.tasks {
        task.proof
            .as_ref()
            .ok_or_else(|| internal("publish_self_check:missing_task_proof"))?
            .validate()
            .map_err(internal_contract)?;
    }
    let _ = canonical_json_bytes(manifest).map_err(internal_contract)?;
    Ok(())
}

fn baseline_compat_for_plan(
    plan: &velnor_actions_contract::Plan,
) -> Result<String, OrchestratorError> {
    let mut pairs: Vec<(&str, &str)> = plan
        .obligations
        .iter()
        .map(|item| (item.task_id.as_str(), item.task_digest.as_str()))
        .collect();
    pairs.sort_unstable();
    let bytes =
        canonical_json_bytes(&serde_json::json!({"label": plan.runner.label, "tasks": pairs}))
            .map_err(internal_contract)?;
    Ok(digest_b3(&bytes))
}

fn baseline_artifact_numeric_id(name: &str) -> u64 {
    let digest = digest_b3(name.as_bytes());
    digest
        .strip_prefix("b3-")
        .and_then(|hex| hex.get(..16))
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
        .max(1)
}

#[cfg(test)]
#[path = "baseline_publish_tests.rs"]
mod baseline_publish_tests;

#[cfg(test)]
#[path = "baseline_publish_branch_tests.rs"]
mod baseline_publish_branch_tests;
