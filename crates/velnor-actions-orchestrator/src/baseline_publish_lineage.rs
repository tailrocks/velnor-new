//! Publisher-side parent loading and task carry construction.

use std::path::Path;

use velnor_actions_contract::{
    BaselineProof, ObligationDecision, WorkflowEvent, canonical_json_bytes, digest_b3,
};

use super::{
    BASELINE_FILENAME, PublishRequest, ci_run_ids, internal, publish_manifest, self_check,
};
use crate::OrchestratorError;
use crate::cover_baseline::provenance_check::{ProvenanceExpectations, validate_provenance};
use crate::merge::BaselineManifest;
use crate::merge::required_evidence::{BaselineTaskEntry, MAX_BASELINE_MANIFEST_BYTES};

/// Load and bind the parent evidence already staged with the plan artifact.
pub(super) fn load_parent_for_publish(
    request: &PublishRequest,
    plan: &velnor_actions_contract::Plan,
    runner_temp: &Path,
) -> Result<Option<BaselineManifest>, OrchestratorError> {
    if !plan
        .obligations
        .iter()
        .any(|item| item.decision == ObligationDecision::CoveredByTrustedBaseline)
    {
        return Ok(None);
    }
    let run_dir = runner_temp.join("velnor").join(&plan.run_key);
    let path = run_dir.join(BASELINE_FILENAME);
    let limit = u64::try_from(MAX_BASELINE_MANIFEST_BYTES).unwrap_or(u64::MAX);
    let text = crate::retrieve_reports::read_staged_text(&path, limit)
        .map_err(|kind| internal(&format!("publish_refused:parent_{kind}")))?;
    let value = crate::internal_plan::snapshot::parse_canonical_json(&text)
        .map_err(|_| internal("publish_refused:parent_malformed"))?;
    let parent: BaselineManifest =
        serde_json::from_value(value).map_err(|_| internal("publish_refused:parent_malformed"))?;
    validate_parent(request, plan, &parent)?;
    Ok(Some(parent))
}

/// Validate the loaded parent against request, plan, and every covered task.
fn validate_parent(
    request: &PublishRequest,
    plan: &velnor_actions_contract::Plan,
    parent: &BaselineManifest,
) -> Result<(), OrchestratorError> {
    let base = request
        .base
        .as_deref()
        .filter(|base| plan.base.as_deref() == Some(*base))
        .filter(|base| parent.source_commit == *base)
        .ok_or_else(|| internal("publish_refused:parent_base_mismatch"))?;
    let slug = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug)
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let branch = request
        .default_branch
        .as_deref()
        .ok_or_else(|| internal("publish_refused:unprotected_ref"))?;
    let bytes = canonical_json_bytes(parent).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_BASELINE_MANIFEST_BYTES {
        return Err(internal("publish_refused:parent_oversize"));
    }
    let digest = digest_b3(&bytes);
    validate_plan_baseline(&plan.baseline, parent, &digest)?;
    let expected = ProvenanceExpectations {
        base: base.to_owned(),
        branch: branch.to_owned(),
        workflow_path: velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned(),
        generator_version: plan.generator.version.clone(),
        generator_sha256: plan.generator.sha256.clone(),
        repository_id: Some(digest_b3(format!("github.com/{slug}").as_bytes())),
        repository_slug: Some(slug),
        repository_conflict: false,
    };
    validate_provenance(parent, &digest, &expected)
        .map_err(|reason| internal(&format!("publish_refused:parent_{reason}")))?;
    validate_covered_tasks(plan, parent, &digest)
}

/// Match the used-plan metadata to the exact canonical parent manifest.
fn validate_plan_baseline(
    baseline: &velnor_actions_contract::PlanBaseline,
    parent: &BaselineManifest,
    digest: &str,
) -> Result<(), OrchestratorError> {
    let value = serde_json::to_value(baseline)
        .map_err(|_| internal("publish_refused:bad_plan_baseline"))?;
    let matches = value["status"] == "used"
        && value["base_commit"] == parent.source_commit
        && value["run_id"] == parent.run_id
        && value["artifact_id"] == parent.artifact_id
        && value["artifact_name"] == parent.artifact_name
        && value["manifest_digest"] == digest;
    if matches {
        Ok(())
    } else {
        Err(internal("publish_refused:plan_parent_mismatch"))
    }
}

/// Match every covered obligation and its proof to one parent task entry.
fn validate_covered_tasks(
    plan: &velnor_actions_contract::Plan,
    parent: &BaselineManifest,
    digest: &str,
) -> Result<(), OrchestratorError> {
    for obligation in &plan.obligations {
        if obligation.decision != ObligationDecision::CoveredByTrustedBaseline {
            continue;
        }
        let proof = obligation
            .baseline_proof
            .as_ref()
            .ok_or_else(|| internal("publish_refused:covered_proof_missing"))?;
        let entry = parent
            .tasks
            .iter()
            .find(|task| task.task_id == obligation.task_id)
            .ok_or_else(|| internal("publish_refused:parent_task_missing"))?;
        let identity_matches = entry.task_digest == obligation.task_digest
            && entry.input_digest == obligation.input_digest
            && entry.closure_digest == obligation.closure_digest
            && proof.source_commit() == parent.source_commit
            && proof.run_id() == entry.proof_run_id
            && proof.artifact_id() == parent.artifact_id
            && proof.artifact_name() == parent.artifact_name
            && proof.manifest_digest() == digest;
        if !identity_matches {
            return Err(internal("publish_refused:parent_task_mismatch"));
        }
    }
    Ok(())
}

/// Build one carried entry with the original proof and current observer.
pub(super) fn carried_entry(
    obligation: &velnor_actions_contract::PlanObligation,
    parent: &BaselineManifest,
    observed_run_id: u64,
) -> Result<BaselineTaskEntry, OrchestratorError> {
    let digest =
        digest_b3(&canonical_json_bytes(parent).map_err(crate::internal::internal_contract)?);
    let source = parent
        .tasks
        .iter()
        .find(|task| task.task_id == obligation.task_id)
        .ok_or_else(|| internal("publish_refused:parent_task_missing"))?;
    let identity_matches = source.task_digest == obligation.task_digest
        && source.input_digest == obligation.input_digest
        && source.closure_digest == obligation.closure_digest;
    if !identity_matches {
        return Err(internal("publish_refused:parent_task_mismatch"));
    }
    if source.external_data.is_some() {
        return Err(internal("publish_refused:unbounded_external_freshness"));
    }
    let carried_from = BaselineProof::new(
        &parent.source_commit,
        parent.run_id,
        parent.artifact_id,
        &parent.artifact_name,
        &digest,
    )
    .map_err(crate::internal::internal_contract)?;
    Ok(BaselineTaskEntry {
        task_id: source.task_id.clone(),
        task_digest: source.task_digest.clone(),
        input_digest: source.input_digest.clone(),
        closure_digest: source.closure_digest.clone(),
        proof_run_id: source.proof_run_id,
        carried_from: Some(carried_from),
        observed_run_id,
        external_data: source.external_data.clone(),
        proof: source.proof.clone(),
    })
}

/// Exact publication preflight used before the planner omits any tasks.
pub(crate) fn carry_candidate_fits(
    plan: &velnor_actions_contract::Plan,
    parent: &BaselineManifest,
    branch: &str,
    repository: Option<&str>,
) -> bool {
    let Ok((run_id, run_attempt)) = ci_run_ids(&plan.run_key) else {
        return false;
    };
    let request = PublishRequest {
        schema: 1,
        op: None,
        event: WorkflowEvent::Push,
        base: plan.base.clone(),
        head: plan.head.clone(),
        repository: repository.map(str::to_owned),
        git_ref: Some(format!("refs/heads/{branch}")),
        default_branch: Some(branch.to_owned()),
    };
    let Ok(candidate) = publish_manifest(&request, plan, run_id, run_attempt, Some(parent)) else {
        return false;
    };
    self_check(&request, &candidate).is_ok()
        && canonical_json_bytes(&candidate)
            .is_ok_and(|bytes| bytes.len() <= MAX_BASELINE_MANIFEST_BYTES)
}
