use super::super::super::{
    github::{GitHubApi, WorkflowMetadata},
    run_identity::{CurrentRun, RunSource},
};
use super::super::{append_output, positive_u64, required};
use super::support::{
    event_json, parse_seed_policy, positive_value, release_job_name_seed, required_value,
    valid_git_sha,
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::time::Duration;
use velnor_actions_contract::{
    CI_WORKFLOW_PATH, PackageReleasePolicy, WORKFLOW_DISPLAY_NAME,
    package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

#[derive(Clone, Copy)]
pub(super) struct SelectionRequest<'a> {
    pub(super) event: &'a Value,
    pub(super) seed: &'a PackageReleasePolicy,
    pub(super) repository_id: u64,
    pub(super) authority: &'a str,
    pub(super) current_run: u64,
    pub(super) current_attempt: u32,
    pub(super) event_name: &'a str,
    pub(super) job_name: &'a str,
    pub(super) manual_run_identity: Option<(u64, u32)>,
    pub(super) deadline: CheckDeadline,
}

/// Select source A and current workflow authority B from authenticated GitHub facts.
/// # Errors
/// Rejects an ineligible CI source, workflow authority, repository, or ancestry.
pub fn select_preview_source_from_env() -> Result<(), OrchestratorError> {
    select()
}

/// Authenticate a successful main CI source and the current workflow authority.
/// This phase runs before checkout and emits only remote identities.
pub(super) fn select() -> Result<(), OrchestratorError> {
    let deadline = CheckDeadline::after(Duration::from_secs(300))
        .map_err(|_| internal("preview_source_deadline"))?;
    deadline
        .scope(|| select_until(deadline))
        .map_err(|_| internal("preview_source_deadline"))?
}

fn select_until(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let seed = parse_seed_policy()?;
    let repository = required("GITHUB_REPOSITORY")?;
    let authority = required("GITHUB_SHA")?;
    let workflow_sha = required("GITHUB_WORKFLOW_SHA")?;
    let ref_name = required("GITHUB_REF")?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    let event_name = required("GITHUB_EVENT_NAME")?;
    let manual_run_identity = if event_name == "workflow_dispatch" {
        Some(parse_manual_run_identity(
            &required("VELNOR_RELEASE_ORIGINAL_RUN_ID")?,
            &required("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT")?,
        )?)
    } else {
        None
    };
    validate_preview_source_context(
        &seed,
        &repository,
        &authority,
        &workflow_sha,
        &ref_name,
        &workflow_ref,
        &event_name,
    )?;
    let event = event_json()?;
    let repository_id = positive_u64("GITHUB_REPOSITORY_ID")?;
    let current_run = positive_u64("GITHUB_RUN_ID")?;
    let current_attempt = u32::try_from(positive_u64("GITHUB_RUN_ATTEMPT")?)
        .map_err(|_| internal("preview_source_context"))?;
    let api = GitHubApi::new(repository);
    select_and_emit_source(
        &api,
        SelectionRequest {
            event: &event,
            seed: &seed,
            repository_id,
            authority: &authority,
            current_run,
            current_attempt,
            event_name: &event_name,
            job_name: &required("GITHUB_JOB")?,
            manual_run_identity,
            deadline,
        },
        append_output,
    )
}

fn validate_preview_source_context(
    seed: &PackageReleasePolicy,
    repository: &str,
    authority: &str,
    workflow_sha: &str,
    ref_name: &str,
    workflow_ref: &str,
    event_name: &str,
) -> Result<(), OrchestratorError> {
    if !matches!(event_name, "workflow_run" | "workflow_dispatch") {
        return Err(internal("preview_source_event_name"));
    }
    if seed.repository != repository
        || seed.channel != PackageReleaseChannel::Preview
        || seed.default_branch != "main"
        || authority != workflow_sha
        || !valid_git_sha(authority)
        || ref_name != "refs/heads/main"
        || workflow_ref
            != format!(
                "{repository}/{}@refs/heads/main",
                seed.channel.workflow_path()
            )
    {
        return Err(internal("preview_source_authority_context"));
    }
    Ok(())
}

fn select_and_emit_source(
    api: &GitHubApi,
    request: SelectionRequest<'_>,
    emit: impl FnOnce(&str) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let SelectionRequest {
        event,
        seed,
        repository_id,
        authority,
        current_run,
        current_attempt,
        event_name,
        job_name,
        manual_run_identity,
        deadline,
    } = request;
    api.verify_repository_identity(repository_id)?;
    let ci_workflow = api.admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
    let (source_sha, ci_run_id, ci_attempt) = source_from_invocation(
        event,
        event_name,
        api,
        repository_id,
        seed,
        &ci_workflow,
        manual_run_identity,
    )?;
    if !valid_git_sha(&source_sha) {
        return Err(internal("preview_source_sha"));
    }

    api.verify_preview_main_ancestry(repository_id, &seed.default_branch, &source_sha, authority)?;
    let ci_source = RunSource {
        repository_id,
        sha: &source_sha,
        branch: &seed.default_branch,
    };
    api.verify_ci_identity(ci_run_id, ci_attempt, ci_source, true)?;

    let workflow =
        api.admitted_workflow(seed.channel.workflow_path(), seed.channel.workflow_name())?;
    let current_source = RunSource {
        repository_id,
        sha: authority,
        branch: &seed.default_branch,
    };
    let role = release_job_name_seed(seed.channel, job_name)?;
    api.verify_current_role(
        &CurrentRun {
            id: current_run,
            attempt: current_attempt,
            source: current_source,
            workflow: &workflow,
            event: event_name,
        },
        &role,
    )?;
    deadline
        .remaining()
        .map_err(|_| internal("preview_source_deadline"))?;
    emit(&format!(
        "source_sha={source_sha}\nworkflow_authority_sha={authority}\nci_run_id={ci_run_id}\nci_attempt={ci_attempt}\n"
    ))
}

fn source_from_invocation(
    event: &Value,
    event_name: &str,
    api: &GitHubApi,
    repository_id: u64,
    seed: &PackageReleasePolicy,
    ci_workflow: &WorkflowMetadata,
    manual_run_identity: Option<(u64, u32)>,
) -> Result<(String, u64, u32), OrchestratorError> {
    match event_name {
        "workflow_run" => {
            source_from_workflow_run_event(event, repository_id, &seed.repository, ci_workflow)
        }
        "workflow_dispatch" => {
            let (id, attempt) = manual_run_identity
                .ok_or_else(|| internal("preview_source_manual_run_identity"))?;
            source_from_workflow_dispatch(api, repository_id, id, attempt)
        }
        _ => Err(internal("preview_source_event_name")),
    }
}

fn parse_manual_run_identity(id: &str, attempt: &str) -> Result<(u64, u32), OrchestratorError> {
    let parse_decimal = |value: &str| {
        value
            .parse::<u64>()
            .ok()
            .filter(|number| *number > 0 && number.to_string() == value)
            .ok_or_else(|| internal("preview_source_manual_run_identity"))
    };
    let id = parse_decimal(id)?;
    let attempt = parse_decimal(attempt)?;
    let attempt =
        u32::try_from(attempt).map_err(|_| internal("preview_source_manual_run_identity"))?;
    Ok((id, attempt))
}

fn source_from_workflow_dispatch(
    api: &GitHubApi,
    repository_id: u64,
    id: u64,
    attempt: u32,
) -> Result<(String, u64, u32), OrchestratorError> {
    let run = api.workflow_run(id, attempt)?;
    if run.id != Some(id)
        || run.run_attempt != Some(attempt)
        || run.head_branch.as_deref() != Some("main")
        || run.event.as_deref() != Some("push")
        || run.status.as_deref() != Some("completed")
        || run.conclusion.as_deref() != Some("success")
        || run
            .repository
            .as_ref()
            .is_none_or(|repo| repo.id != repository_id)
        || run
            .head_repository
            .as_ref()
            .is_none_or(|repo| repo.id != repository_id)
    {
        return Err(internal("preview_source_manual_run_identity"));
    }
    let source_sha = run
        .head_sha
        .ok_or_else(|| internal("preview_source_manual_run_identity"))?;
    if !valid_git_sha(&source_sha) {
        return Err(internal("preview_source_manual_run_identity"));
    }
    Ok((source_sha, id, attempt))
}

fn source_from_workflow_run_event(
    event: &Value,
    repository_id: u64,
    repository: &str,
    ci_workflow: &WorkflowMetadata,
) -> Result<(String, u64, u32), OrchestratorError> {
    let run = &event["workflow_run"];
    if ci_workflow.id == 0
        || ci_workflow.path != CI_WORKFLOW_PATH
        || ci_workflow.name != WORKFLOW_DISPLAY_NAME
        || event["action"].as_str() != Some("completed")
        || event["repository"]["id"].as_u64() != Some(repository_id)
        || event["repository"]["full_name"].as_str() != Some(repository)
        || run["repository"]["id"].as_u64() != Some(repository_id)
        || run["repository"]["full_name"].as_str() != Some(repository)
        || event["workflow"]["id"].as_u64() != Some(ci_workflow.id)
        || event["workflow"]["name"].as_str() != Some(ci_workflow.name.as_str())
        || event["workflow"]["path"].as_str() != Some(ci_workflow.path.as_str())
        || run["workflow_id"].as_u64() != Some(ci_workflow.id)
        || run["head_repository"]["id"].as_u64() != Some(repository_id)
        || run["head_repository"]["full_name"].as_str() != Some(repository)
        || run["head_branch"].as_str() != Some("main")
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("completed")
        || run["conclusion"].as_str() != Some("success")
        || run["path"].as_str() != Some(ci_workflow.path.as_str())
        || run["name"].as_str() != Some(ci_workflow.name.as_str())
    {
        return Err(internal("preview_source_event_identity"));
    }
    Ok((
        required_value(&run["head_sha"], "preview_source_event_identity")?,
        positive_value(&run["id"], "preview_source_event_identity")?,
        u32::try_from(positive_value(
            &run["run_attempt"],
            "preview_source_event_identity",
        )?)
        .map_err(|_| internal("preview_source_event_identity"))?,
    ))
}

#[cfg(test)]
#[path = "preview_source_admission_tests.rs"]
mod tests;
