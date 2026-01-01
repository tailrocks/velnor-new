//! Expected artifact producer identities derived from the authoritative plan.

use super::{
    ArtifactBuildExpectation, ArtifactBuildIdentity, ArtifactBuildProducer, ArtifactBuildProvider,
    ArtifactBuildRunContext,
};
use crate::workflow::{HOSTED_SUFFIX, Plan, SCALE_SUFFIX};
use std::collections::BTreeSet;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract_config::VERIFICATION_TASK_JOB_PREFIX;

/// Return the sorted union of provider identities represented in a plan.
#[must_use]
pub fn artifact_plan_providers(plan: &Plan) -> Vec<ArtifactBuildProvider> {
    plan.artifact_tasks
        .iter()
        .flat_map(|task| task.providers.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Derive the exact provider/task obligations from a validated plan.
///
/// The plan contains each task's exact provider inventory. `providers` must
/// equal their sorted union; each task contributes expectations only for its
/// own providers. Source and plan digests are computed from the plan itself.
/// # Errors
pub fn expected_artifact_builds(
    plan: &Plan,
    context: &ArtifactBuildRunContext,
    providers: &[ArtifactBuildProvider],
) -> Result<Vec<ArtifactBuildExpectation>, ContractError> {
    plan.validate()?;
    if plan.artifact_tasks.is_empty() {
        return Ok(Vec::new());
    }
    validate_providers(plan, providers)?;
    if plan.run_key != format!("r{}-a{}", context.run_id, context.run_attempt) {
        return Err(ContractError::identity("artifact.run", "run_key_mismatch"));
    }
    validate_run_context(context)?;
    let plan_digest = super::canonical_plan_digest(plan)?;
    let capacity = plan
        .artifact_tasks
        .iter()
        .try_fold(0_usize, |sum, task| sum.checked_add(task.providers.len()))
        .ok_or_else(|| ContractError::identity("artifact.plan", "inventory_overflow"))?;
    let mut expected = Vec::with_capacity(capacity);
    for provider in providers {
        for item in &plan.artifact_tasks {
            if !item.providers.contains(provider) {
                continue;
            }
            let paired = item.providers.len() > 1;
            let (workflow_job_id, workflow_job_name) = match item.producer {
                ArtifactBuildProducer::MatrixBuild => {
                    (provider.workflow_job_id(paired).to_owned(), String::new())
                }
                ArtifactBuildProducer::VerificationTask => (
                    verification_task_job_id(*provider, &item.task.id, paired),
                    verification_task_job_name(*provider, &item.task.id, paired),
                ),
            };
            let identity = ArtifactBuildIdentity {
                repository_id: context.repository_id.clone(),
                repository: context.repository.clone(),
                source_sha: plan.head.clone(),
                plan_digest: plan_digest.clone(),
                run_id: context.run_id.clone(),
                run_attempt: context.run_attempt,
                workflow_job_id,
                provider: *provider,
                task_id: item.task.id.clone(),
            };
            super::identity::validate_identity(&identity)?;
            expected.push(ArtifactBuildExpectation {
                identity,
                workflow_job_name,
                producer: item.producer,
                outputs: item.task.outputs.clone(),
            });
        }
    }
    Ok(expected)
}

fn validate_providers(
    plan: &Plan,
    providers: &[ArtifactBuildProvider],
) -> Result<(), ContractError> {
    if providers.is_empty() {
        return Err(ContractError::identity(
            "artifact.providers",
            "empty_provider_inventory",
        ));
    }
    let mut previous = None;
    for provider in providers {
        if previous.is_some_and(|value| value >= *provider) {
            return Err(ContractError::identity(
                "artifact.providers",
                "providers_must_be_sorted_unique",
            ));
        }
        previous = Some(*provider);
    }
    if artifact_plan_providers(plan) != providers {
        return Err(ContractError::identity(
            "artifact.providers",
            "provider_scope_mismatch",
        ));
    }
    Ok(())
}

fn verification_task_job_id(
    provider: ArtifactBuildProvider,
    task_id: &str,
    paired: bool,
) -> String {
    let base = format!("{VERIFICATION_TASK_JOB_PREFIX}{task_id}");
    if !paired {
        return base;
    }
    match provider {
        ArtifactBuildProvider::GithubHosted => format!("{base}{HOSTED_SUFFIX}"),
        ArtifactBuildProvider::VelnorScaleSet => format!("{base}{SCALE_SUFFIX}"),
    }
}

fn verification_task_job_name(
    provider: ArtifactBuildProvider,
    task_id: &str,
    paired: bool,
) -> String {
    let base = format!("Verify {task_id}");
    match (provider, paired) {
        (ArtifactBuildProvider::GithubHosted, false) => base,
        (ArtifactBuildProvider::GithubHosted, true) => {
            format!("{base} / GitHub hosted / Linux x64")
        }
        (ArtifactBuildProvider::VelnorScaleSet, _) => {
            format!("{base} / Velnor Scale Set / Linux x64")
        }
    }
}

fn validate_run_context(context: &ArtifactBuildRunContext) -> Result<(), ContractError> {
    if context
        .repository_id
        .parse::<u64>()
        .ok()
        .is_none_or(|id| id == 0 || id.to_string() != context.repository_id)
    {
        return Err(ContractError::identity(
            "artifact.repository_id",
            "bad_repository_id",
        ));
    }
    if !super::valid_repository_slug(&context.repository) {
        return Err(ContractError::identity(
            "artifact.repository",
            "bad_repository_slug",
        ));
    }
    if context
        .run_id
        .parse::<u64>()
        .ok()
        .is_none_or(|id| id == 0 || id.to_string() != context.run_id)
        || context.run_attempt == 0
    {
        return Err(ContractError::identity("artifact.run", "bad_run_context"));
    }
    Ok(())
}
