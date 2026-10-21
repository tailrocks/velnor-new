//! Build the completed producer evidence from exact plan artifacts.

use std::path::{Path, PathBuf};

#[cfg(test)]
use velnor_actions_contract::task_report_id_for_task;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendObservation, QualificationCacheDirective,
    QualificationCacheLaneLayerReceipt, QualificationCacheLaneReceipt,
    QualificationCacheLayerReceipt, QualificationCacheOwnerReceipt,
    QualificationCacheProducerEvidence, QualificationCacheRestore, QualificationCacheSave,
};
use velnor_actions_contract::{ExecuteTaskRef, Plan, canonical_json_bytes, digest_b3};

use super::SOURCE_EVIDENCE_FILENAME;
use super::context::{AuthenticatedProducerContext, read_authenticated_context};
use super::types::QualificationSourceEvidenceDocument;
use super::validate_runner_directories;
mod files;
#[path = "receipt_writer/validation.rs"]
mod validation;
use crate::OrchestratorError;
use crate::internal::internal;
#[cfg(test)]
use files::parse_canonical;
use files::{
    artifact_root, read_canonical, read_checked_bytes, receipt_path, reports_directory,
    required_path,
};
use validation::{owner_artifact_id, unique_obligation, validate_source};

const RECEIPT_FILENAME: &str = "qualification-cache-receipt.json";
const OWNER_EVIDENCE_DIRECTORY: &str = "qualification-cache/owners";
const MAX_OWNER_BYTES: u64 = 262_144;
const MAX_SOURCE_BYTES: u64 = 65_536;
const MAX_REPORT_BYTES: u64 = crate::retrieve_reports::MAX_STAGED_REPORT_BYTES;

/// Validate downloaded producer evidence and write one bounded receipt file.
///
/// # Errors
///
/// Returns an error when a valid qualification plan lacks required runner, source, report, or cache owner evidence.
pub(super) fn write_receipt() -> Result<PathBuf, OrchestratorError> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let Some(plan) = load_receipt_plan(&run_key, &runner_temp)? else {
        return Ok(PathBuf::new());
    };
    if plan.qualification.is_none() {
        return Ok(PathBuf::new());
    }
    validate_runner_directories(&runner_temp, &run_key, None)?;
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
    let mut owners = read_owners(&reports, &plan, &directive)?;
    apply_backend_observations(&mut owners, &plan, &authenticated, &runner_temp, &run_key)?;
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

fn load_receipt_plan(run_key: &str, runner_temp: &Path) -> Result<Option<Plan>, OrchestratorError> {
    match crate::task_report::load_plan(run_key, runner_temp) {
        Ok(plan) => Ok(Some(plan)),
        Err(OrchestratorError::Io { problem, .. }) if problem == "not_found" => Ok(None),
        Err(OrchestratorError::Contract { .. }) => Ok(None),
        Err(OrchestratorError::Internal { problem })
            if problem == "unparsable_plan"
                || problem == "report_run_mismatch"
                || problem.starts_with("unreadable_plan:") =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn apply_backend_observations(
    owners: &mut [QualificationCacheOwnerReceipt],
    plan: &Plan,
    authenticated: &AuthenticatedProducerContext,
    runner_temp: &Path,
    run_key: &str,
) -> Result<(), OrchestratorError> {
    let run_dir = runner_temp.join("velnor").join(run_key);
    let before_bytes = read_checked_bytes(
        &run_dir
            .join(crate::qualification_cache_backend::QUALIFICATION_CACHE_BACKEND_BEFORE_FILENAME),
        16 * 1024 * 1024,
    )?;
    let before = crate::qualification_cache_backend::parse_qualification_cache_before(
        &before_bytes,
        &authenticated.dispatch,
    )?;
    before.validate_for(&authenticated.dispatch, run_key)?;
    let after_bytes = read_checked_bytes(
        &run_dir
            .join(crate::qualification_cache_backend::QUALIFICATION_CACHE_BACKEND_AFTER_FILENAME),
        16 * 1024 * 1024,
    )?;
    let after = crate::qualification_cache_backend::parse_qualification_cache_after(
        &after_bytes,
        &authenticated.dispatch,
    )?;
    if plan.run_key != run_key {
        return Err(internal("qualification_receipt_snapshot_run_mismatch"));
    }
    for owner in owners {
        for layer in &mut owner.layers {
            if let Some(key) = layer.restore.requested_key.as_deref() {
                layer.restore.matched_cache =
                    before.observation_for(key, &authenticated.dispatch.git_ref)?;
            } else {
                layer.restore.matched_cache = QualificationCacheBackendObservation::NotQueried;
            }
            if let Some(key) = layer.save.requested_key.as_deref() {
                layer.save.before = before.observation_for(key, &authenticated.dispatch.git_ref)?;
                layer.save.after = after.observation_for(key, &authenticated.dispatch.git_ref)?;
            } else {
                layer.save.before = QualificationCacheBackendObservation::NotQueried;
                layer.save.after = QualificationCacheBackendObservation::NotQueried;
            }
        }
    }
    Ok(())
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
        let bytes = read_checked_bytes(&path, MAX_OWNER_BYTES)?;
        let worker = crate::QualificationCacheWorkerOwnerFacts::parse_canonical(&bytes)?;
        let receipt = project_worker_owner(worker);
        if receipt.owner_id != owner.owner_id
            || receipt.platform_id != owner.platform_id
            || receipt.member_digest != owner.member_digest
            || receipt.members != owner.members
            || receipt.layers.len() != owner.layers.len()
            || receipt
                .layers
                .iter()
                .zip(&owner.layers)
                .any(|(actual, expected)| {
                    actual.layer != expected.layer
                        || actual.active != expected.active
                        || actual.identity_digest != expected.identity_digest
                })
        {
            return Err(internal("qualification_receipt_owner_binding_mismatch"));
        }
        owners.push(receipt);
    }
    Ok(owners)
}

fn project_worker_owner(
    worker: crate::QualificationCacheWorkerOwnerFacts,
) -> QualificationCacheOwnerReceipt {
    QualificationCacheOwnerReceipt {
        owner_id: worker.owner_id,
        platform_id: worker.platform_id,
        member_digest: worker.member_digest,
        members: worker.members,
        layers: worker
            .layers
            .into_iter()
            .map(|layer| QualificationCacheLayerReceipt {
                owner_id: layer.owner_id,
                platform_id: layer.platform_id,
                member_digest: layer.member_digest,
                layer: layer.layer,
                active: layer.active,
                identity_digest: layer.identity_digest,
                runtime_identity: layer.runtime_identity,
                state_digest: layer.state_digest,
                restore: QualificationCacheRestore {
                    requested_key: layer.restore.requested_key,
                    matched_key: layer.restore.matched_key,
                    matched_cache: QualificationCacheBackendObservation::NotQueried,
                    result: layer.restore.result,
                },
                save: QualificationCacheSave {
                    requested_key: layer.save.requested_key,
                    action: layer.save.action,
                    before: QualificationCacheBackendObservation::NotQueried,
                    after: QualificationCacheBackendObservation::NotQueried,
                },
            })
            .collect(),
    }
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

#[cfg(test)]
#[path = "receipt_writer_tests.rs"]
mod tests;
