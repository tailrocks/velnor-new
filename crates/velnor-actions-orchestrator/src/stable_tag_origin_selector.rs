//! Read-only source/CI selection; permanent tag writers reauthenticate these hints.

use super::super::{
    github::{GitHubApi, GitRefResponse, TagObjectResponse},
    run_identity::{CurrentRun, RunSource, run_matches},
};
use super::{artifacts, attempt, authorization_for_context, positive, recovery, required};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, PackageReleasePlan, WORKFLOW_DISPLAY_NAME,
    package_release::PackageReleaseChannel,
};

/// Read-only selection of the exact fresh or original stable source.
pub const SELECT_STABLE_RELEASE_SOURCE_OP: &str = "select-stable-release-source-v1";

/// Select a fresh current source or the original authority of a same-run retry.
/// # Errors
/// Refuses stale/foreign workflows, missing full CI and changed original intents.
pub fn select_stable_release_source_from_env() -> Result<(), OrchestratorError> {
    super::super::artifact_zip::within_operation(select)
}

fn select() -> Result<(), OrchestratorError> {
    if required("VELNOR_INTERNAL_OP")? != SELECT_STABLE_RELEASE_SOURCE_OP
        || required("GITHUB_JOB")? != "stable-eligibility"
        || required("GITHUB_EVENT_NAME")? != "workflow_dispatch"
    {
        return Err(internal("stable_selector_role"));
    }
    let sha = required("GITHUB_SHA")?;
    let (_, plan) = crate::release_policy_admission::resolve_policy_from_env(&sha)?;
    let repository_id = positive("GITHUB_REPOSITORY_ID")?;
    if plan.channel != PackageReleaseChannel::Stable
        || required("GITHUB_REPOSITORY")? != plan.repository
        || required("GITHUB_REF")? != format!("refs/heads/{}", plan.default_branch)
        || required("GITHUB_WORKFLOW_SHA")? != sha
        || required("GITHUB_WORKFLOW_REF")?
            != format!(
                "{}/{}@refs/heads/{}",
                plan.repository,
                plan.channel.workflow_path(),
                plan.default_branch
            )
    {
        return Err(internal("stable_selector_source"));
    }
    let api = GitHubApi::new(plan.repository.clone());
    api.verify_repository(repository_id)?;
    let run_id = positive("GITHUB_RUN_ID")?;
    let current_attempt = attempt("GITHUB_RUN_ATTEMPT")?;
    let source = RunSource {
        repository_id,
        sha: &sha,
        branch: &plan.default_branch,
    };
    let workflow =
        api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
    api.verify_current_role(
        &CurrentRun {
            id: run_id,
            attempt: current_attempt,
            source,
            workflow: &workflow,
            event: "workflow_dispatch",
        },
        "Authenticate stable source and complete CI",
    )?;
    let (ci_run_id, ci_attempt) = if let Some((old_attempt, reference)) =
        recovery::prior_reference(&api, run_id, current_attempt)?
    {
        resumed_ci(
            &api,
            &plan,
            artifacts::ArtifactOwner {
                source,
                run_id,
                attempt: old_attempt,
            },
            &reference,
        )?
    } else {
        fresh_ci(&api, &plan, source)?
    };
    append(&format!(
        "source_sha={sha}\nworkflow_authority_sha={sha}\nci_run_id={ci_run_id}\nci_attempt={ci_attempt}\ncommit_count=0\n"
    ))
}

fn resumed_ci(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    owner: artifacts::ArtifactOwner<'_>,
    reference: &super::ArtifactRef,
) -> Result<(u64, u32), OrchestratorError> {
    let workflow =
        api.admitted_workflow(plan.channel.workflow_path(), plan.channel.workflow_name())?;
    let original = artifacts::read_prior_intent(
        api,
        artifacts::ArtifactOwner {
            source: owner.source,
            run_id: owner.run_id,
            attempt: owner.attempt,
        },
        reference,
    )?;
    let context = &original.authorization.context;
    if context.run_id != owner.run_id
        || context.run_attempt != owner.attempt
        || context.repository_id != owner.source.repository_id
        || context.source_sha != owner.source.sha
        || context.source_ref != format!("refs/heads/{}", plan.default_branch)
    {
        return Err(internal("stable_selector_original_context"));
    }
    let expected = authorization_for_context(plan, context.clone())?;
    artifacts::validate_intent(&original, &expected)?;
    let old_run = api.workflow_run(owner.run_id, owner.attempt)?;
    if !run_matches(
        &old_run,
        &workflow,
        owner.run_id,
        owner.attempt,
        owner.source,
        "workflow_dispatch",
    ) || old_run.status.as_deref() != Some("completed")
    {
        return Err(internal("stable_selector_original_run"));
    }
    api.verify_ci_identity(
        context.ci_run_id,
        context.ci_run_attempt,
        owner.source,
        false,
    )?;
    let object: TagObjectResponse =
        api.get_json(&api.route(&format!("git/tags/{}", original.tag.object_sha)))?;
    if object.sha != original.tag.object_sha {
        return Err(internal("stable_selector_original_object"));
    }
    super::tag::validate_created_object(
        &object,
        &original.tag.name,
        &original.tag.message,
        owner.source.sha,
    )?;
    Ok((context.ci_run_id, context.ci_run_attempt))
}

fn fresh_ci(
    api: &GitHubApi,
    plan: &PackageReleasePlan,
    source: RunSource<'_>,
) -> Result<(u64, u32), OrchestratorError> {
    let head: GitRefResponse = api.get_json(&api.route(&format!(
        "git/ref/heads/{}",
        super::super::github::encode_segment(&plan.default_branch)
    )))?;
    if head.ref_name != format!("refs/heads/{}", plan.default_branch)
        || head.object.object_type != "commit"
        || head.object.sha != source.sha
    {
        return Err(internal("stable_selector_current_main"));
    }
    let workflow = api.admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
    let ci = api.latest_source_ci(&workflow, source)?;
    let id = ci
        .id
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("stable_selector_ci_id"))?;
    let attempt = ci
        .run_attempt
        .filter(|attempt| *attempt > 0)
        .ok_or_else(|| internal("stable_selector_ci_attempt"))?;
    api.verify_ci_identity(id, attempt, source, true)?;
    Ok((id, attempt))
}

fn append(value: &str) -> Result<(), OrchestratorError> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(required("GITHUB_OUTPUT")?)
        .map_err(|_| internal("stable_selector_output"))?;
    file.write_all(value.as_bytes())
        .map_err(|_| internal("stable_selector_output"))
}
