//! Build the completed producer evidence from exact plan artifacts.

use std::path::{Path, PathBuf};

use velnor_actions_contract::workflow::{
    MatrixReport, MatrixStatus, QualificationCacheDirective, QualificationCacheLaneLayerReceipt,
    QualificationCacheLaneReceipt, QualificationCacheOwnerReceipt,
    QualificationCacheProducerEvidence, TaskReport, TaskStatus,
};
use velnor_actions_contract::{
    ExecuteTaskRef, Plan, canonical_json_bytes, digest_b3, report_id_for_matrix,
    task_report_id_for_task, validate_artifact_id,
};

use super::SOURCE_EVIDENCE_FILENAME;
use super::context::{AuthenticatedProducerContext, read_authenticated_context};
use super::types::QualificationSourceEvidenceDocument;
use super::validate_runner_directories;
mod files;
use crate::OrchestratorError;
use crate::internal::internal;
use files::{
    artifact_root, parse_canonical, read_canonical, read_checked_bytes, receipt_path,
    reports_directory, required_path,
};

const RECEIPT_FILENAME: &str = "qualification-cache-receipt.json";
const OWNER_EVIDENCE_DIRECTORY: &str = "qualification-cache/owners";
const MAX_OWNER_BYTES: u64 = 262_144;
const MAX_SOURCE_BYTES: u64 = 65_536;
const MAX_REPORT_BYTES: u64 = crate::retrieve_reports::MAX_STAGED_REPORT_BYTES;

/// Validate downloaded producer evidence and write one bounded receipt file.
///
/// # Errors
///
/// Returns an error if plan, runner, source, report, or cache-owner evidence is missing or differs.
pub(super) fn write_receipt() -> Result<PathBuf, OrchestratorError> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    validate_runner_directories(&runner_temp, &run_key, None)?;
    let plan = crate::task_report::load_plan(&run_key, &runner_temp)?;
    let workspace = required_path("GITHUB_WORKSPACE")?;
    let prepared = crate::prepare::prepare(&workspace)?;
    let authenticated = read_authenticated_context(&prepared)?;
    if authenticated.run_key != run_key || plan.run_key != run_key {
        return Err(internal("qualification_receipt_run_key_mismatch"));
    }
    let admission =
        crate::qualification_resolver::read_qualification_admission_for_plan(&plan, &runner_temp)?;
    let directive = QualificationCacheDirective::for_plan(&plan, admission.as_ref())
        .map_err(crate::internal::internal_contract)?
        .ok_or_else(|| internal("qualification_receipt_plan_required"))?;
    let reports = reports_directory(&runner_temp, &run_key);
    let owners = read_owners(&reports, &plan, &directive)?;
    let lanes = read_lanes(&reports, &plan, &directive, &authenticated, &owners)?;
    let evidence = QualificationCacheProducerEvidence {
        plan_id: plan.plan_id.clone(),
        campaign: directive.campaign.clone(),
        phase: directive.phase,
        source_sha: authenticated.producer.source_sha.clone(),
        configuration_digest: directive.configuration_digest.clone(),
        source_delta: admission
            .as_ref()
            .and_then(|value| value.source_delta().cloned()),
        predecessor: admission
            .as_ref()
            .map(velnor_actions_contract::QualificationCacheAdmission::receipt_link)
            .transpose()
            .map_err(crate::internal::internal_contract)?,
        lanes,
        owners,
    };
    let document = velnor_actions_contract::workflow::QualificationCacheReceiptArtifactDocument {
        schema: 4,
        producer: authenticated.producer,
        evidence,
    };
    let bytes = document
        .to_bounded_json(&plan, admission.as_ref())
        .map_err(crate::internal::internal_contract)?;
    let output = receipt_path(&runner_temp, &run_key);
    crate::exclusive_write::write_private_exclusive(
        &output,
        &bytes,
        "qualification_cache_receipt",
    )?;
    Ok(output)
}

fn read_owners(
    reports: &Path,
    plan: &Plan,
    directive: &QualificationCacheDirective,
) -> Result<Vec<QualificationCacheOwnerReceipt>, OrchestratorError> {
    let mut owners = Vec::with_capacity(directive.owners.len());
    for owner in &directive.owners {
        let artifact_id = owner_artifact_id(plan, &owner.owner_id)?;
        let marker =
            PathBuf::from(OWNER_EVIDENCE_DIRECTORY).join(format!("{}.json", owner.owner_id));
        let root = artifact_root(reports, &artifact_id, &marker)?;
        let path = root.join(&marker);
        let receipt: QualificationCacheOwnerReceipt = read_canonical(&path, MAX_OWNER_BYTES)?;
        if receipt.owner_id != owner.owner_id
            || receipt.platform_id != owner.platform_id
            || receipt.member_digest != owner.member_digest
            || receipt.members != owner.members
        {
            return Err(internal("qualification_receipt_owner_binding_mismatch"));
        }
        owners.push(receipt);
    }
    Ok(owners)
}

fn read_lanes(
    reports: &Path,
    plan: &Plan,
    directive: &QualificationCacheDirective,
    authenticated: &AuthenticatedProducerContext,
    owners: &[QualificationCacheOwnerReceipt],
) -> Result<Vec<QualificationCacheLaneReceipt>, OrchestratorError> {
    let mut lanes = Vec::with_capacity(plan.matrix.include.len());
    for entry in &plan.matrix.include {
        let artifact_id = owner_artifact_id(plan, &entry.job_id)?;
        let marker = PathBuf::from(&entry.matrix_key).join(SOURCE_EVIDENCE_FILENAME);
        let root = artifact_root(reports, &artifact_id, &marker)?;
        let source_path = root.join(&marker);
        let source: QualificationSourceEvidenceDocument =
            read_canonical(&source_path, MAX_SOURCE_BYTES)?;
        let obligation = unique_obligation(plan, &entry.task_id)?;
        validate_source(&source, plan, entry, obligation, authenticated, &root)?;
        let lane_directive = directive
            .lanes
            .iter()
            .find(|lane| lane.matrix_key == entry.matrix_key)
            .ok_or_else(|| internal("qualification_receipt_lane_directive_missing"))?;
        let layers = lane_directive
            .layers
            .iter()
            .map(|layer| QualificationCacheLaneLayerReceipt {
                layer: layer.layer,
                active: layer.active,
                identity_digest: layer.identity_digest.clone(),
                owner_id: layer.owner_id.clone(),
            })
            .collect::<Vec<_>>();
        let owner = owners
            .iter()
            .find(|owner| owner.owner_id == entry.job_id)
            .ok_or_else(|| internal("qualification_receipt_owner_missing"))?;
        let mut useful_layers = owner
            .layers
            .iter()
            .filter(|layer| layer.active)
            .map(|layer| {
                layer
                    .state_digest
                    .as_ref()
                    .map(|digest| (layer.layer, digest.as_str()))
                    .ok_or_else(|| internal("qualification_receipt_owner_state_missing"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        useful_layers.sort_by_key(|(layer, _)| *layer);
        let useful_state = canonical_json_bytes(&(
            source.evidence.closure.observed_closure_digest.as_str(),
            useful_layers,
        ))
        .map_err(crate::internal::internal_contract)?;
        let mut completed_task_ids = entry
            .execute_task_ids
            .tasks
            .values()
            .flat_map(|task| match task {
                ExecuteTaskRef::Single(task_id) => vec![task_id.clone()],
                ExecuteTaskRef::Shards(shards) => shards.clone(),
            })
            .collect::<Vec<_>>();
        completed_task_ids.sort();
        lanes.push(QualificationCacheLaneReceipt {
            matrix_key: entry.matrix_key.clone(),
            stack_id: entry.stack_id.clone(),
            task_id: entry.task_id.clone(),
            completed_task_ids,
            closure_digest: source.evidence.closure.observed_closure_digest.clone(),
            useful_state_digest: digest_b3(&useful_state),
            layers,
        });
    }
    lanes.sort_by(|left, right| left.matrix_key.cmp(&right.matrix_key));
    Ok(lanes)
}

fn validate_source(
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
    evidence: &super::types::QualificationSourceEvidence,
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

fn owner_artifact_id(plan: &Plan, owner_id: &str) -> Result<String, OrchestratorError> {
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

fn unique_obligation<'a>(
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

#[cfg(test)]
#[path = "receipt_writer_tests.rs"]
mod tests;
