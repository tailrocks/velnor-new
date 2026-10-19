//! Read-only current-main and latest exact-source CI admission.
use super::super::{
    github::{GitHubApi, GitRefResponse, RepositoryResponse, WorkflowRunList},
    run_identity::{RunSource, ci_run_is_success},
};
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME};

pub(super) fn repository_id(api: &GitHubApi, repository: &str) -> Result<u64, OrchestratorError> {
    let observed: RepositoryResponse = api.get_json(&format!("repos/{repository}"))?;
    if observed.id == 0 || observed.full_name.as_deref() != Some(repository) {
        return Err(internal("tap_continuation_repository"));
    }
    Ok(observed.id)
}

pub(super) fn head(api: &GitHubApi, branch: &str) -> Result<String, OrchestratorError> {
    let observed: GitRefResponse = api.get_json(&api.route(&format!(
        "git/ref/heads/{}",
        super::super::github::encode_segment(branch)
    )))?;
    if observed.ref_name != format!("refs/heads/{branch}")
        || observed.object.object_type != "commit"
        || !super::super::identity::is_git_sha(&observed.object.sha)
    {
        return Err(internal("tap_continuation_main_identity"));
    }
    Ok(observed.object.sha)
}

pub(super) fn latest_ci(
    api: &GitHubApi,
    source: RunSource<'_>,
) -> Result<Option<(u64, u32)>, OrchestratorError> {
    let workflow = api.admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
    let first: WorkflowRunList = api.get_json(&api.route(&format!(
        "actions/workflows/{}/runs?head_sha={}&branch={}&event=push&per_page=100&page=1",
        workflow.id,
        super::super::github::encode_segment(source.sha),
        super::super::github::encode_segment(source.branch)
    )))?;
    if first.workflow_runs.is_empty() {
        return Ok(None);
    }
    let run = api.latest_source_ci(&workflow, source)?;
    let id = run
        .id
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("tap_continuation_ci_identity"))?;
    let attempt = run
        .run_attempt
        .filter(|n| *n > 0)
        .ok_or_else(|| internal("tap_continuation_ci_identity"))?;
    if !ci_run_is_success(&run, id, attempt, source, &workflow) {
        return Ok(None);
    }
    api.verify_ci_identity(id, attempt, source, true)?;
    Ok(Some((id, attempt)))
}

pub(crate) fn verify_current_source(
    repository: &str,
    branch: &str,
    sha: &str,
) -> Result<(), OrchestratorError> {
    let api = GitHubApi::new(repository.to_owned());
    verify_source(&api, repository, branch, sha)
}

fn verify_source(
    api: &GitHubApi,
    repository: &str,
    branch: &str,
    sha: &str,
) -> Result<(), OrchestratorError> {
    let id = repository_id(api, repository)?;
    if head(api, branch)? != sha {
        return Err(internal("tap_source_release_superseded"));
    }
    if latest_ci(
        api,
        RunSource {
            repository_id: id,
            sha,
            branch,
        },
    )?
    .is_none()
    {
        return Err(internal("tap_source_latest_ci_not_successful"));
    }
    if head(api, branch)? != sha {
        return Err(internal("tap_source_release_superseded"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "package_release_continuation_source_tests.rs"]
mod tests;
