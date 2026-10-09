//! Exact job and downloaded-output reconciliation against the plan.

use std::collections::{BTreeMap, BTreeSet};

use super::identity::validate_identity;
use super::{
    ArtifactBuildExpectation, ArtifactBuildObservation, ArtifactBuildProvider,
    ArtifactBuildRunContext,
};
use crate::workflow::{JobConclusion, Plan};
use velnor_actions_contract::errors::ContractError;

/// Require one successful, checksum-matching result for every expected pair.
///
/// Extra, missing, duplicate, skipped, cancelled, and failed jobs are all
/// rejected. Expected tasks come only from the validated plan; provider lanes
/// must be supplied from the selected workflow routing mode.
/// # Errors
pub fn reconcile_artifact_builds(
    plan: &Plan,
    context: &ArtifactBuildRunContext,
    providers: &[ArtifactBuildProvider],
    observed: &[ArtifactBuildObservation],
) -> Result<(), ContractError> {
    let expected = super::expected_artifact_builds(plan, context, providers)?;
    let expected_map = expectation_map(&expected)?;
    let mut seen = BTreeSet::new();
    let mut seen_api_jobs = BTreeSet::new();
    let mut seen_api_artifacts = BTreeSet::new();
    if observed.len() != expected_map.len() {
        return Err(ContractError::identity(
            "artifact.jobs",
            "expected_job_count_mismatch",
        ));
    }
    for observation in observed {
        let key = (
            observation.identity.provider,
            observation.identity.task_id.clone(),
        );
        if !seen.insert(key.clone()) {
            return Err(ContractError::identity("artifact.jobs", "duplicate_job"));
        }
        let Some(expectation) = expected_map.get(&key) else {
            return Err(ContractError::identity("artifact.jobs", "unexpected_job"));
        };
        validate_observation(
            plan,
            context,
            expectation,
            observation,
            &mut seen_api_jobs,
            &mut seen_api_artifacts,
        )?;
    }
    if seen.len() != expected_map.len() {
        return Err(ContractError::identity("artifact.jobs", "missing_job"));
    }
    Ok(())
}

fn validate_observation(
    plan: &Plan,
    context: &ArtifactBuildRunContext,
    expectation: &ArtifactBuildExpectation,
    observation: &ArtifactBuildObservation,
    seen_api_jobs: &mut BTreeSet<u64>,
    seen_api_artifacts: &mut BTreeSet<u64>,
) -> Result<(), ContractError> {
    validate_api_inventory(
        context,
        expectation,
        observation,
        seen_api_jobs,
        seen_api_artifacts,
    )?;
    if observation.api_job_status != "completed" || observation.conclusion != JobConclusion::Success
    {
        return Err(ContractError::identity(
            "artifact.job.conclusion",
            observation.conclusion.as_str(),
        ));
    }
    validate_provider_runner(observation)?;
    let task = plan
        .artifact_tasks
        .iter()
        .find(|item| item.task.id == observation.identity.task_id)
        .ok_or_else(|| ContractError::identity("artifact.task", "unknown_task"))?;
    if observation.identity != expectation.identity
        || expectation.outputs != task.task.outputs
        || !task.providers.contains(&observation.identity.provider)
    {
        return Err(ContractError::identity(
            "artifact.job.identity",
            "identity_or_plan_mismatch",
        ));
    }
    validate_report_and_download(expectation, task, observation)
}

fn validate_api_inventory(
    context: &ArtifactBuildRunContext,
    expectation: &ArtifactBuildExpectation,
    observation: &ArtifactBuildObservation,
    seen_api_jobs: &mut BTreeSet<u64>,
    seen_api_artifacts: &mut BTreeSet<u64>,
) -> Result<(), ContractError> {
    let api_job_id = observation
        .api_job_id
        .filter(|id| *id > 0)
        .ok_or_else(|| ContractError::identity("artifact.api_inventory", "missing_api_job_id"))?;
    let api_artifact_id = observation
        .api_artifact_id
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            ContractError::identity("artifact.api_inventory", "missing_api_artifact_id")
        })?;
    if observation
        .api_artifact_size_bytes
        .is_none_or(|size| size == 0)
        || observation.api_artifact_expired != Some(false)
        || !seen_api_jobs.insert(api_job_id)
        || !seen_api_artifacts.insert(api_artifact_id)
    {
        return Err(ContractError::identity(
            "artifact.api_inventory",
            "missing_or_duplicate_api_identity",
        ));
    }
    if observation.api_run_id != context.run_id
        || observation.api_head_sha != expectation.identity.source_sha
        || observation.api_artifact_run_id.as_deref() != Some(context.run_id.as_str())
        || observation.api_artifact_repository_id.as_deref() != Some(context.repository_id.as_str())
        || observation.api_artifact_head_sha.as_deref()
            != Some(expectation.identity.source_sha.as_str())
    {
        return Err(ContractError::identity(
            "artifact.api_inventory",
            "run_repository_or_source_mismatch",
        ));
    }
    Ok(())
}

fn validate_provider_runner(observation: &ArtifactBuildObservation) -> Result<(), ContractError> {
    if observation.identity.provider == ArtifactBuildProvider::VelnorScaleSet
        && (observation.api_runner_id.is_none_or(|id| id == 0)
            || observation
                .api_runner_name
                .as_deref()
                .is_none_or(str::is_empty)
            || observation.api_runner_group_id.is_none_or(|id| id == 0)
            || observation
                .api_runner_group_name
                .as_deref()
                .is_none_or(str::is_empty)
            || observation.api_runner_labels.is_empty())
    {
        return Err(ContractError::identity(
            "artifact.runner",
            "missing_scale_set_runner_identity",
        ));
    }
    Ok(())
}

fn validate_report_and_download(
    expectation: &ArtifactBuildExpectation,
    task: &super::ArtifactBuildTaskPlan,
    observation: &ArtifactBuildObservation,
) -> Result<(), ContractError> {
    let result = observation
        .result
        .as_ref()
        .ok_or_else(|| ContractError::identity("artifact.result", "missing_result"))?;
    let expected_name = super::artifact_name(&expectation.identity)?;
    if observation.api_job_name != expectation.expected_workflow_job_name()
        || observation.api_artifact_name.as_deref() != Some(expected_name.as_str())
    {
        return Err(ContractError::identity(
            "artifact.api_inventory",
            "api_job_or_artifact_name_mismatch",
        ));
    }
    result.validate_for(&task.task, &expectation.identity)?;
    validate_downloaded_outputs(
        &result.outputs,
        &expectation.outputs,
        &observation.downloaded_outputs,
    )
}

pub(super) fn validate_downloaded_outputs(
    result: &[super::ArtifactBuildFile],
    expected: &[velnor_actions_contract_config::ArtifactBuildOutput],
    downloaded: &[super::DownloadedArtifactOutput],
) -> Result<(), ContractError> {
    if result.len() != expected.len() || downloaded.len() != expected.len() {
        return Err(ContractError::identity(
            "artifact.downloads",
            "download_inventory_mismatch",
        ));
    }
    for ((reported, declared), actual) in result.iter().zip(expected).zip(downloaded) {
        if actual.output_id != declared.id
            || actual.path != declared.path
            || actual.output_id != reported.output_id
            || actual.path != reported.path
        {
            return Err(ContractError::identity(
                "artifact.downloads",
                "download_inventory_mismatch",
            ));
        }
        if actual.size_bytes == 0
            || actual.size_bytes != reported.size_bytes
            || actual.size_bytes > declared.max_bytes
            || actual.digest != reported.digest
            || velnor_actions_contract::canonical::validate_digest(&actual.digest).is_err()
        {
            return Err(ContractError::identity(
                "artifact.download",
                format!("download_content_mismatch:{}", declared.id),
            ));
        }
    }
    Ok(())
}

fn expectation_map(
    expected: &[ArtifactBuildExpectation],
) -> Result<BTreeMap<(ArtifactBuildProvider, String), &ArtifactBuildExpectation>, ContractError> {
    let mut values = BTreeMap::new();
    for item in expected {
        validate_identity(&item.identity)?;
        let key = (item.identity.provider, item.identity.task_id.clone());
        if values.insert(key, item).is_some() {
            return Err(ContractError::identity(
                "artifact.plan",
                "duplicate_expectation",
            ));
        }
    }
    Ok(values)
}
