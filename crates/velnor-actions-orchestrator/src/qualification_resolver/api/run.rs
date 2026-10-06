//! Exact workflow-run attempt metadata from the Actions API.

use velnor_actions_contract::{QualificationCacheRunMetadata, QualificationRunRef};

use crate::OrchestratorError;
use crate::internal::internal;

use super::client::GitHub;

#[derive(serde::Deserialize)]
pub(super) struct RunResponse {
    id: Option<u64>,
    run_attempt: Option<u32>,
    event: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    head_branch: Option<String>,
    head_sha: Option<String>,
    path: Option<String>,
    head_repository: Option<HeadRepository>,
}

#[derive(serde::Deserialize)]
struct HeadRepository {
    full_name: Option<String>,
}

pub(super) fn load_run_metadata(
    client: &GitHub<'_>,
    run: QualificationRunRef,
) -> Result<QualificationCacheRunMetadata, OrchestratorError> {
    let route = format!(
        "repos/{}/actions/runs/{}/attempts/{}",
        client.repository(),
        run.run_id,
        run.run_attempt
    );
    let response: RunResponse = GitHub::json(client.catalog(), &route)?;
    validate_run(&response, client, run)
}

fn validate_run(
    value: &RunResponse,
    client: &GitHub<'_>,
    run: QualificationRunRef,
) -> Result<QualificationCacheRunMetadata, OrchestratorError> {
    let head_sha = required(value.head_sha.as_deref(), "qualification_run_head_sha")?;
    let event = required(value.event.as_deref(), "qualification_run_event")?;
    let conclusion = required(value.conclusion.as_deref(), "qualification_run_conclusion")?;
    let head_branch = required(value.head_branch.as_deref(), "qualification_run_branch")?;
    let path = required(value.path.as_deref(), "qualification_run_workflow")?;
    let head_repo = value
        .head_repository
        .as_ref()
        .and_then(|repo| repo.full_name.as_deref())
        .ok_or_else(|| internal("qualification_run_repository"))?;
    if !requested_attempt_matches(value, run)
        || value.status.as_deref() != Some("completed")
        || event != "workflow_dispatch"
        || conclusion != "success"
        || head_branch != client.default_branch()
        || head_repo != client.repository()
        || !valid_sha(head_sha)
        || !workflow_path_matches(path, client.default_branch())
    {
        return Err(internal("qualification_run_authority_mismatch"));
    }
    Ok(QualificationCacheRunMetadata {
        repository: client.repository().to_owned(),
        default_branch: client.default_branch().to_owned(),
        git_ref: format!("refs/heads/{}", client.default_branch()),
        ref_protected: true,
        workflow_path_ref: format!(
            "{}@{}",
            velnor_actions_workflow_renderer::render::WORKFLOW_PATH,
            client.default_branch()
        ),
        workflow_ref: format!(
            "{}/{}@refs/heads/{}",
            client.repository(),
            velnor_actions_workflow_renderer::render::WORKFLOW_PATH,
            client.default_branch()
        ),
        workflow_sha: head_sha.to_owned(),
        head_sha: head_sha.to_owned(),
        event: event.to_owned(),
        conclusion: conclusion.to_owned(),
        run,
    })
}

fn requested_attempt_matches(value: &RunResponse, run: QualificationRunRef) -> bool {
    value.id == Some(run.run_id) && value.run_attempt == Some(run.run_attempt)
}

fn required<'a>(value: Option<&'a str>, error: &'static str) -> Result<&'a str, OrchestratorError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(error))
}

fn workflow_path_matches(value: &str, branch: &str) -> bool {
    let path = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    if value == path {
        return true;
    }
    let ref_suffix = value.strip_prefix(&format!("{path}@"));
    ref_suffix.is_some_and(|suffix| suffix == branch || suffix == format!("refs/heads/{branch}"))
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
