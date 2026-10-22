use super::{positive_value, required, required_value, valid_git_sha};
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use velnor_actions_contract::{PackageReleasePolicy, package_release::PackageReleaseChannel};

use super::super::super::{
    github::{GitHubApi, WorkflowMetadata},
    run_identity::RunSource,
};

pub(super) fn source_from_invocation(
    api: &GitHubApi,
    event: &Value,
    repository_id: u64,
    seed: &PackageReleasePolicy,
    event_name: &str,
) -> Result<(String, u64, u32, String), OrchestratorError> {
    match event_name {
        "workflow_run" => {
            api.verify_repository_identity(repository_id)?;
            let workflow = api.admitted_workflow(
                velnor_actions_contract::CI_WORKFLOW_PATH,
                velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            )?;
            let (source, run, attempt) =
                source_from_workflow_run_event(event, repository_id, &seed.repository, &workflow)?;
            Ok((source, run, attempt, required("GITHUB_SHA")?))
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
                required("GITHUB_SHA")?,
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
