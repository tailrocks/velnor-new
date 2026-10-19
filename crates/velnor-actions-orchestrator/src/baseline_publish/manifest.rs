//! Prepare the artifact from task and matrix reports that passed the checks.

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    ManifestTaskProof, MatrixEntry, ObligationDecision, Plan, PlanObligation, TaskReport,
    TaskStatus, Trust, WorkflowEvent, canonical_json_bytes, digest_b3,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

use super::PublishRequest;

/// One task row for schema 2, from this run only.
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

/// The existing baseline artifact schema, with data from this run's reports.
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

/// Prepare the publication artifact and check its data.
pub(super) fn build_manifest(
    request: &PublishRequest,
    plan: &Plan,
    run_id: u64,
    run_attempt: u64,
    reports: &[TaskReport],
) -> Result<(Vec<u8>, String), OrchestratorError> {
    if plan.generator.sha256.is_empty() || plan.generator.sha256.chars().any(char::is_uppercase) {
        return Err(internal("publish_refused:generator_unverifiable"));
    }
    if reports.len() != plan.matrix.include.len() {
        return Err(internal("publish_refused:task_report_inventory_mismatch"));
    }
    let mut tasks = Vec::with_capacity(plan.obligations.len());
    for obligation in &plan.obligations {
        tasks.push(build_task(obligation, plan, run_id, reports)?);
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
    let manifest = BaselineManifest {
        schema: Plan::SCHEMA,
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
        artifact_name: name.clone(),
        tasks,
        parent: None,
        expires_at_unix: None,
    };
    self_check(&manifest)?;
    let bytes = canonical_json_bytes(&manifest).map_err(internal_contract)?;
    Ok((bytes, name))
}

fn build_task(
    obligation: &PlanObligation,
    plan: &Plan,
    run_id: u64,
    reports: &[TaskReport],
) -> Result<PublishedTask, OrchestratorError> {
    if obligation.decision != ObligationDecision::Execute {
        return Err(internal("publish_refused:nonexecuted_obligation"));
    }
    let matched = validate_obligation_reports(obligation, plan, reports)?;
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
    Ok(PublishedTask {
        task_id: obligation.task_id.clone(),
        task_digest: obligation.task_digest.clone(),
        input_digest: obligation.input_digest.clone(),
        closure_digest: obligation.closure_digest.clone(),
        proof_run_id: run_id,
        observed_run_id: run_id,
        carried_from: None,
        external_data: None,
        proof: Some(proof),
    })
}

fn validate_obligation_reports<'a>(
    obligation: &PlanObligation,
    plan: &'a Plan,
    reports: &'a [TaskReport],
) -> Result<Vec<(&'a MatrixEntry, &'a TaskReport)>, OrchestratorError> {
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
    for entry in entries {
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
        validate_lane_report(obligation, plan, entry, report)?;
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
    Ok(matched)
}

fn validate_lane_report(
    obligation: &PlanObligation,
    plan: &Plan,
    entry: &MatrixEntry,
    report: &TaskReport,
) -> Result<(), OrchestratorError> {
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
        .map_err(|_| internal("publish_refused:task_report_invalid"))
}

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

fn baseline_compat_for_plan(plan: &Plan) -> Result<String, OrchestratorError> {
    let mut pairs: Vec<(&str, &str)> = plan
        .obligations
        .iter()
        .map(|item| (item.task_id.as_str(), item.task_digest.as_str()))
        .collect();
    pairs.sort_unstable();
    let bytes = canonical_json_bytes(&serde_json::json!({
        "label": plan.runner.label,
        "tasks": pairs
    }))
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
