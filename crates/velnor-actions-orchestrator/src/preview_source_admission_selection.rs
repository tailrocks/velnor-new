use super::{
    positive_value, release_job_name_seed, required_value, source_checkout_lfs, valid_git_sha,
};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use velnor_actions_contract::{PackageReleasePolicy, package_release::PackageReleaseChannel};
use velnor_actions_mise::CheckDeadline;

use super::super::super::{
    github::{GitHubApi, WorkflowMetadata},
    run_identity::{CurrentRun, RunSource},
};

pub(super) struct SelectionRequest<'a> {
    pub(super) event: &'a Value,
    pub(super) seed: &'a PackageReleasePolicy,
    pub(super) repository_id: u64,
    pub(super) authority: &'a str,
    pub(super) current_run: u64,
    pub(super) current_attempt: u32,
    pub(super) event_name: &'a str,
    pub(super) job_name: &'a str,
    pub(super) deadline: CheckDeadline,
}

pub(super) fn select_and_emit_source(
    api: &GitHubApi,
    request: &SelectionRequest<'_>,
    emit: impl FnOnce(&str) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let (source_sha, ci_run_id, ci_attempt) = source_from_invocation(
        api,
        request.event,
        request.repository_id,
        request.seed,
        request.event_name,
    )?;
    if !valid_git_sha(&source_sha) || !valid_git_sha(request.authority) {
        return Err(internal("preview_source_sha"));
    }
    api.verify_preview_main_ancestry(
        request.repository_id,
        &request.seed.default_branch,
        &source_sha,
        request.authority,
    )?;
    let ci_source = RunSource {
        repository_id: request.repository_id,
        sha: &source_sha,
        branch: &request.seed.default_branch,
    };
    api.verify_ci_identity(ci_run_id, ci_attempt, ci_source, true)?;

    let workflow = api.admitted_workflow(
        request.seed.channel.workflow_path(),
        request.seed.channel.workflow_name(),
    )?;
    let current_source = RunSource {
        repository_id: request.repository_id,
        sha: request.authority,
        branch: &request.seed.default_branch,
    };
    let role = release_job_name_seed(request.seed.channel, request.job_name)?;
    api.verify_current_role(
        &CurrentRun {
            id: request.current_run,
            attempt: request.current_attempt,
            source: current_source,
            workflow: &workflow,
            event: request.event_name,
        },
        &role,
    )?;
    let checkout_lfs = source_checkout_lfs(api, &source_sha)?;
    request
        .deadline
        .remaining()
        .map_err(|_| internal("preview_source_deadline"))?;
    emit(&format!(
        "source_sha={source_sha}\nworkflow_authority_sha={}\nci_run_id={ci_run_id}\nci_attempt={ci_attempt}\ncheckout_lfs={checkout_lfs}\n",
        request.authority
    ))
}

pub(super) fn source_from_invocation(
    api: &GitHubApi,
    event: &Value,
    repository_id: u64,
    seed: &PackageReleasePolicy,
    event_name: &str,
) -> Result<(String, u64, u32), OrchestratorError> {
    match event_name {
        "workflow_run" => {
            api.verify_repository_identity(repository_id)?;
            let workflow = api.admitted_workflow(
                velnor_actions_contract::CI_WORKFLOW_PATH,
                velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            )?;
            let (source, run, attempt) =
                source_from_workflow_run_event(event, repository_id, &seed.repository, &workflow)?;
            Ok((source, run, attempt))
        }
        "workflow_dispatch" => {
            let signed = super::super::super::recovery::verify_preview_original_for_source_selection(
                seed,
                super::super::super::recovery::PreviewRecoveryVerificationStage::SourceSelection,
            )?;
            api.verify_repository_identity(repository_id)?;
            let receipt = signed.receipt();
            let source_sha = receipt.source.commit_sha.clone();
            let workflow = api.admitted_workflow(
                velnor_actions_contract::CI_WORKFLOW_PATH,
                velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            )?;
            let latest = api.latest_source_ci(
                &workflow,
                RunSource {
                    repository_id,
                    sha: &source_sha,
                    branch: &seed.default_branch,
                },
            )?;
            Ok((
                source_sha,
                latest
                    .id
                    .filter(|id| *id > 0)
                    .ok_or_else(|| internal("preview_source_latest_ci_identity"))?,
                latest
                    .run_attempt
                    .filter(|attempt| *attempt > 0)
                    .ok_or_else(|| internal("preview_source_latest_ci_identity"))?,
            ))
        }
        _ => Err(internal("preview_source_event_name")),
    }
}

pub(super) fn source_from_workflow_run_event(
    event: &Value,
    repository_id: u64,
    repository: &str,
    workflow: &WorkflowMetadata,
) -> Result<(String, u64, u32), OrchestratorError> {
    let run = &event["workflow_run"];
    if workflow.id == 0
        || workflow.path != velnor_actions_contract::CI_WORKFLOW_PATH
        || workflow.name != velnor_actions_contract::WORKFLOW_DISPLAY_NAME
        || event["action"].as_str() != Some("completed")
        || event["repository"]["id"].as_u64() != Some(repository_id)
        || event["repository"]["full_name"].as_str() != Some(repository)
        || run["repository"]["id"].as_u64() != Some(repository_id)
        || run["repository"]["full_name"].as_str() != Some(repository)
        || event["workflow"]["id"].as_u64() != Some(workflow.id)
        || event["workflow"]["name"].as_str() != Some(workflow.name.as_str())
        || event["workflow"]["path"].as_str() != Some(workflow.path.as_str())
        || run["workflow_id"].as_u64() != Some(workflow.id)
        || run["head_repository"]["id"].as_u64() != Some(repository_id)
        || run["head_repository"]["full_name"].as_str() != Some(repository)
        || run["head_branch"].as_str() != Some("main")
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("completed")
        || run["conclusion"].as_str() != Some("success")
        || run["path"].as_str() != Some(workflow.path.as_str())
        || run["name"].as_str() != Some(workflow.name.as_str())
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

pub(super) fn validate_preview_source_context(
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
