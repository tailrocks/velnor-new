//! Implementation for the pure scoped identity join.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_workflow::{
    MatrixEntry, NamedCheckLaneVariant, Plan, TaskRuntimeReceipt, canonical_plan_digest,
};

use super::scoped_compare::{
    ActionsAttemptArtifactView, ActionsAttemptJobView, CompleteActionsAttemptView,
    ScopedCompareError, ScopedCompareLane, ScopedCompareLaneBinding, ScopedCompareRequest,
    ScopedCompareResult,
};
use super::{TaskReportOutputFanIn, TaskReportOutputOrigin};

/// Join each typed named-check lane to its exact scope-bound provider rows.
///
/// Only typed lane pairs are emitted; unrelated plan entries are ignored.
/// Each emitted lane must have an exact runtime receipt, successful producer
/// output, a unique Check Run mapping, and a same-run/repository/head artifact
/// with the plan's exact artifact name. The sidecar's producer census is only
/// internally validated here: binding its `expected_workflow_job_keys` to the
/// workflow dependency graph is a separate requirement. Likewise, accepting a
/// complete-view trait value does not prove who supplied it.
///
/// No runner-known or parity claim is returned. The lane value is copied only
/// from `MatrixEntry::lane_variant`; provider names and runner labels are never
/// considered.
///
/// # Errors
/// Returns a scoped error when any plan, receipt, producer output, provider
/// identity, or selected lane binding is missing, malformed, or inconsistent.
pub fn bind_scoped_compare<P: CompleteActionsAttemptView>(
    request: ScopedCompareRequest<'_>,
    plan: &Plan,
    receipts: &[TaskRuntimeReceipt],
    producer_outputs: &TaskReportOutputFanIn,
    provider: &P,
) -> Result<ScopedCompareResult, ScopedCompareError> {
    let repository = validate_request(request)?;
    validate_plan_scope(plan, request)?;
    let plan_digest = canonical_plan_digest(plan).map_err(|_| ScopedCompareError::InvalidPlan)?;
    let outputs = validate_outputs(producer_outputs)?;
    validate_provider_scope(provider, request, &repository, plan)?;
    validate_output_scope(&outputs, request, &repository, plan, &plan_digest, provider)?;
    let indexes = index_provider(provider)?;
    let receipt_index = index_receipts(plan, receipts, request, &repository, &plan_digest)?;
    let lanes = bind_lanes(plan, &outputs, &indexes, &receipt_index)?;
    if lanes.is_empty() {
        return Err(ScopedCompareError::NoTypedLanes);
    }
    Ok(ScopedCompareResult {
        repository_id: provider.repository_id(),
        repository,
        run_id: request.run_id,
        attempt: request.attempt,
        head_sha: plan.head.clone(),
        plan_digest,
        lanes,
    })
}

fn validate_request(request: ScopedCompareRequest<'_>) -> Result<String, ScopedCompareError> {
    let repository =
        velnor_actions_orchestrator_core::origin::validate_repository_slug(request.repository)
            .ok_or(ScopedCompareError::InvalidRequest)?;
    if request.run_id <= 0 || request.attempt == 0 {
        return Err(ScopedCompareError::InvalidRequest);
    }
    Ok(repository)
}

fn validate_plan_scope(
    plan: &Plan,
    request: ScopedCompareRequest<'_>,
) -> Result<(), ScopedCompareError> {
    plan.validate()
        .map_err(|_| ScopedCompareError::InvalidPlan)?;
    if !is_commit_sha(&plan.head) {
        return Err(ScopedCompareError::InvalidPlan);
    }
    if plan.run_key != format!("r{}-a{}", request.run_id, request.attempt) {
        return Err(ScopedCompareError::PlanScopeMismatch);
    }
    Ok(())
}

fn validate_outputs(
    outputs: &TaskReportOutputFanIn,
) -> Result<TaskReportOutputFanIn, ScopedCompareError> {
    let value =
        serde_json::to_value(outputs).map_err(|_| ScopedCompareError::InvalidProducerOutputs)?;
    let parsed = TaskReportOutputFanIn::parse_value(value)
        .map_err(|_| ScopedCompareError::InvalidProducerOutputs)?;
    let mut producer_by_check_run = BTreeMap::new();
    for producer in &parsed.producers {
        if let Some(previous_key) = producer_by_check_run.insert(
            producer.check_run_id.get(),
            producer.workflow_job_key.as_str(),
        ) && previous_key != producer.workflow_job_key
        {
            return Err(ScopedCompareError::InvalidProducerOutputs);
        }
    }
    Ok(parsed)
}

fn validate_output_scope<P: CompleteActionsAttemptView>(
    outputs: &TaskReportOutputFanIn,
    request: ScopedCompareRequest<'_>,
    repository: &str,
    plan: &Plan,
    plan_digest: &str,
    provider: &P,
) -> Result<(), ScopedCompareError> {
    let output_repository =
        velnor_actions_orchestrator_core::origin::validate_repository_slug(&outputs.run.repository);
    if outputs.origin != TaskReportOutputOrigin::GithubCom
        || output_repository.as_deref() != Some(repository)
        || outputs.run.repository_id != provider.repository_id().to_string()
        || outputs.run.run_id != request.run_id.to_string()
        || outputs.run.run_attempt != request.attempt
        || outputs.head_sha != plan.head
        || outputs.plan_digest != plan_digest
    {
        return Err(ScopedCompareError::ProducerScopeMismatch);
    }
    Ok(())
}

fn validate_provider_scope<P: CompleteActionsAttemptView>(
    provider: &P,
    request: ScopedCompareRequest<'_>,
    repository: &str,
    plan: &Plan,
) -> Result<(), ScopedCompareError> {
    let provider_repository = velnor_actions_orchestrator_core::origin::validate_repository_slug(
        provider.repository_full_name(),
    );
    if provider.repository_id() <= 0
        || provider_repository.as_deref() != Some(repository)
        || provider.workflow_run_id() != request.run_id
        || provider.attempt() != request.attempt
        || provider.head_sha() != plan.head
    {
        return Err(ScopedCompareError::ProviderScopeMismatch);
    }
    if provider.run_status() != "completed" || provider.run_conclusion() != Some("success") {
        return Err(ScopedCompareError::ProviderRunUnsuccessful);
    }
    Ok(())
}

struct ProviderIndexes<'a, P: CompleteActionsAttemptView> {
    jobs_by_check_run: BTreeMap<i64, &'a P::Job>,
    artifacts_by_id: BTreeMap<i64, &'a P::Artifact>,
}

fn index_provider<P: CompleteActionsAttemptView>(
    provider: &P,
) -> Result<ProviderIndexes<'_, P>, ScopedCompareError> {
    let mut jobs_by_check_run = BTreeMap::new();
    let mut job_ids = BTreeSet::new();
    for job in provider.jobs() {
        if job.actions_job_id() <= 0
            || job.workflow_run_id() != provider.workflow_run_id()
            || job.head_sha() != provider.head_sha()
            || !job_ids.insert(job.actions_job_id())
        {
            return Err(ScopedCompareError::InvalidProviderInventory);
        }
        if let Some(check_run_id) = job.check_run_id()
            && (check_run_id <= 0 || jobs_by_check_run.insert(check_run_id, job).is_some())
        {
            return Err(ScopedCompareError::InvalidProviderInventory);
        }
    }
    let mut artifacts_by_id = BTreeMap::new();
    for artifact in provider.artifacts() {
        if artifact.artifact_id() <= 0
            || artifact.workflow_run_id() != provider.workflow_run_id()
            || artifact.repository_id() != provider.repository_id()
            || artifact.head_sha() != provider.head_sha()
            || artifacts_by_id
                .insert(artifact.artifact_id(), artifact)
                .is_some()
        {
            return Err(ScopedCompareError::InvalidProviderInventory);
        }
    }
    Ok(ProviderIndexes {
        jobs_by_check_run,
        artifacts_by_id,
    })
}

fn index_receipts<'a>(
    plan: &Plan,
    receipts: &'a [TaskRuntimeReceipt],
    request: ScopedCompareRequest<'_>,
    repository: &str,
    plan_digest: &str,
) -> Result<BTreeMap<&'a str, &'a TaskRuntimeReceipt>, ScopedCompareError> {
    let mut index = BTreeMap::new();
    for receipt in receipts {
        let mut entries = plan
            .matrix
            .include
            .iter()
            .filter(|entry| entry.id == receipt.matrix_id);
        let entry = entries.next().ok_or(ScopedCompareError::InvalidReceipt)?;
        if entries.next().is_some()
            || receipt.validate_for_plan_entry(plan, entry).is_err()
            || velnor_actions_orchestrator_core::origin::validate_repository_slug(
                &receipt.repository,
            )
            .as_deref()
                != Some(repository)
            || receipt.run_id != request.run_id.to_string()
            || receipt.run_attempt != request.attempt
            || receipt.source_sha != plan.head
            || receipt.plan_digest != plan_digest
            || index.insert(receipt.matrix_id.as_str(), receipt).is_some()
        {
            return Err(ScopedCompareError::InvalidReceipt);
        }
    }
    Ok(index)
}

fn bind_lanes<P: CompleteActionsAttemptView>(
    plan: &Plan,
    outputs: &TaskReportOutputFanIn,
    provider: &ProviderIndexes<'_, P>,
    receipts: &BTreeMap<&str, &TaskRuntimeReceipt>,
) -> Result<Vec<ScopedCompareLaneBinding>, ScopedCompareError> {
    let output_by_key: BTreeMap<&str, _> = outputs
        .producers
        .iter()
        .map(|output| (output.workflow_job_key.as_str(), output))
        .collect();
    let mut lanes = Vec::new();
    for entry in &plan.matrix.include {
        let Some(lane) = declared_lane(entry.lane_variant) else {
            continue;
        };
        let receipt = receipts
            .get(entry.id.as_str())
            .ok_or(ScopedCompareError::MissingLaneInput)?;
        let output = output_by_key
            .get(entry.job_id.as_str())
            .ok_or(ScopedCompareError::MissingLaneInput)?;
        let job = provider
            .jobs_by_check_run
            .get(&output.check_run_id.get())
            .ok_or(ScopedCompareError::JobBindingMismatch)?;
        validate_selected_job(*job, output.check_run_id.get())?;
        let artifact = provider
            .artifacts_by_id
            .get(&output.artifact_id.get())
            .ok_or(ScopedCompareError::ArtifactBindingMismatch)?;
        validate_selected_artifact(*artifact, entry, receipt)?;
        lanes.push(ScopedCompareLaneBinding {
            matrix_id: entry.id.clone(),
            matrix_key: entry.matrix_key.clone(),
            task_id: entry.task_id.clone(),
            workflow_job_key: entry.job_id.clone(),
            lane,
            task_report_id: receipt.task_report_id.clone(),
            artifact_name: entry.artifact_id.clone(),
            artifact_id: output.artifact_id.get(),
            check_run_id: output.check_run_id.get(),
            actions_job_id: job.actions_job_id(),
        });
    }
    Ok(lanes)
}

fn validate_selected_job<J: ActionsAttemptJobView>(
    job: &J,
    expected_check_run_id: i64,
) -> Result<(), ScopedCompareError> {
    if job.check_run_id() != Some(expected_check_run_id) {
        return Err(ScopedCompareError::JobBindingMismatch);
    }
    if job.status() != "completed" || job.conclusion() != Some("success") {
        return Err(ScopedCompareError::JobUnsuccessful);
    }
    Ok(())
}

fn validate_selected_artifact<A: ActionsAttemptArtifactView>(
    artifact: &A,
    entry: &MatrixEntry,
    receipt: &TaskRuntimeReceipt,
) -> Result<(), ScopedCompareError> {
    if artifact.expired()
        || artifact.artifact_name() != entry.artifact_id
        || artifact.artifact_name() != receipt.report_artifact_name
    {
        return Err(ScopedCompareError::ArtifactBindingMismatch);
    }
    Ok(())
}

fn declared_lane(variant: Option<NamedCheckLaneVariant>) -> Option<ScopedCompareLane> {
    match variant? {
        NamedCheckLaneVariant::Hosted => Some(ScopedCompareLane::Hosted),
        NamedCheckLaneVariant::ScaleSet => Some(ScopedCompareLane::ScaleSet),
    }
}

fn is_commit_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
