//! Validate source evidence and reports against the authenticated plan.

use std::path::Path;

use velnor_actions_contract::workflow::{MatrixReport, MatrixStatus, TaskReport, TaskStatus};
use velnor_actions_contract::{
    Plan, canonical_json_bytes, digest_b3, report_id_for_matrix, task_report_id_for_task,
    validate_artifact_id,
};

use super::super::context::AuthenticatedProducerContext;
use super::super::types::{QualificationSourceEvidence, QualificationSourceEvidenceDocument};
use super::files::read_checked_bytes;
use super::{MAX_REPORT_BYTES, files::parse_canonical};
use crate::OrchestratorError;
use crate::internal::internal;

pub(super) fn validate_source(
    document: &QualificationSourceEvidenceDocument,
    plan: &Plan,
    entry: &velnor_actions_contract::MatrixEntry,
    obligation: &velnor_actions_contract::PlanObligation,
    authenticated: &AuthenticatedProducerContext,
    artifact_root: &Path,
) -> Result<(), OrchestratorError> {
    let evidence = &document.evidence;
    if document.schema != 1
        || document.producer != authenticated.producer
        || evidence.plan_id != plan.plan_id
        || evidence.matrix_key != entry.matrix_key
        || evidence.job_id != entry.job_id
        || evidence.task_id != entry.task_id
        || evidence.planned_task_digest != entry.task_digest
        || evidence.planned_input_digest != entry.input_digest
        || evidence.planned_closure_digest != obligation.closure_digest
        || evidence.source_sha != authenticated.producer.source_sha
        || evidence.generator.version != plan.generator.version
        || evidence.generator.target != plan.generator.target
        || evidence.generator.sha256 != plan.generator.sha256
        || evidence.cache_evidence_complete
        || evidence.closure.matrix_key != entry.matrix_key
        || evidence.closure.task_id != entry.task_id
        || evidence.closure.observed_task_digest != entry.task_digest
        || evidence.closure.observed_input_digest != entry.input_digest
        || evidence.closure.observed_closure_digest != obligation.closure_digest
    {
        return Err(internal("qualification_receipt_source_binding_mismatch"));
    }
    validate_reports(artifact_root, plan, entry, evidence)
}

fn validate_reports(
    root: &Path,
    plan: &Plan,
    entry: &velnor_actions_contract::MatrixEntry,
    evidence: &QualificationSourceEvidence,
) -> Result<(), OrchestratorError> {
    let task_id = task_report_id_for_task(&plan.run_key, &entry.matrix_key, &entry.task_digest)
        .map_err(crate::internal::internal_contract)?;
    let task_path = root
        .join(&entry.matrix_key)
        .join("tasks")
        .join(format!("{task_id}.json"));
    let matrix_path = root.join(&entry.matrix_key).join("matrix-report.json");
    let task_bytes = read_checked_bytes(&task_path, MAX_REPORT_BYTES)?;
    let matrix_bytes = read_checked_bytes(&matrix_path, MAX_REPORT_BYTES)?;
    let task: TaskReport = parse_canonical(&task_bytes)?;
    task.validate_for_plan_entry(entry)
        .map_err(crate::internal::internal_contract)?;
    task.validate_outputs_declared(&entry.declared_outputs)
        .map_err(crate::internal::internal_contract)?;
    if task.task_report_id != task_id
        || task.run_key != plan.run_key
        || task.event != plan.event
        || task.trust != plan.trust
        || task.task_id != entry.task_id
        || task.task_digest != entry.task_digest
        || task.status != TaskStatus::Executed
        || task.exit_code != 0
        || evidence.task_report.report_id != task_id
        || evidence.task_report.digest_b3 != digest_b3(&task_bytes)
        || evidence.task_report.status != TaskStatus::Executed
        || evidence.task_report.exit_code != 0
    {
        return Err(internal("qualification_receipt_task_report_mismatch"));
    }
    let matrix: MatrixReport = parse_canonical(&matrix_bytes)?;
    matrix
        .validate()
        .map_err(crate::internal::internal_contract)?;
    let expected = crate::task_report::single_task_aggregate(plan, entry, &task)
        .map_err(crate::internal::internal_contract)?;
    let expected_bytes =
        canonical_json_bytes(&expected).map_err(crate::internal::internal_contract)?;
    let matrix_id = report_id_for_matrix(&plan.run_key, &entry.matrix_key)
        .map_err(crate::internal::internal_contract)?;
    if matrix_bytes != expected_bytes
        || matrix.report_id != matrix_id
        || matrix.status != MatrixStatus::Passed
        || evidence.matrix_report.report_id != matrix_id
        || evidence.matrix_report.digest_b3 != digest_b3(&matrix_bytes)
        || evidence.matrix_report.status != MatrixStatus::Passed
    {
        return Err(internal("qualification_receipt_matrix_report_mismatch"));
    }
    Ok(())
}

pub(super) fn owner_artifact_id(plan: &Plan, owner_id: &str) -> Result<String, OrchestratorError> {
    let mut entries = plan
        .matrix
        .include
        .iter()
        .filter(|entry| entry.job_id == owner_id);
    let first = entries
        .next()
        .ok_or_else(|| internal("qualification_receipt_owner_artifact_missing"))?;
    validate_artifact_id(&first.artifact_id).map_err(crate::internal::internal_contract)?;
    if entries.any(|entry| entry.artifact_id != first.artifact_id) {
        return Err(internal("qualification_receipt_owner_artifact_ambiguous"));
    }
    Ok(first.artifact_id.clone())
}

pub(super) fn unique_obligation<'a>(
    plan: &'a Plan,
    task_id: &str,
) -> Result<&'a velnor_actions_contract::PlanObligation, OrchestratorError> {
    let mut obligations = plan
        .obligations
        .iter()
        .filter(|item| item.task_id == task_id);
    let obligation = obligations
        .next()
        .ok_or_else(|| internal("qualification_receipt_obligation_missing"))?;
    if obligations.next().is_some()
        || obligation.decision != velnor_actions_contract::ObligationDecision::Execute
    {
        return Err(internal("qualification_receipt_obligation_invalid"));
    }
    Ok(obligation)
}
