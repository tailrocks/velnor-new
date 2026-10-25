use velnor_actions_mise::CheckDeadline;

use super::{GitHubApi, run_identity::RunSource};
use crate::{OrchestratorError, internal::internal};

const SDK_REPOSITORY: &str = "tailrocks/velnor-new";
const SDK_REPOSITORY_ID: u64 = 1_390_620_900;
const SDK_WORKFLOW_PATH: &str = ".github/workflows/product-release.yml";
const SDK_WORKFLOW_NAME: &str = "Velnor product releases";
const SDK_ATTEST_MANIFEST_JOB: &str =
    "Run velnor-actions generator release graph / attest-manifest";
const SDK_PUBLISH_JOB: &str = "Run velnor-actions generator release graph / publish-generator";
const SDK_ADOPTION_CALLEE_PATH: &str = ".github/workflows/homebrew-adoption.yml";

pub(crate) struct PublicSdkProducerSelection {
    pub(crate) repository: String,
    pub(crate) repository_id: u64,
    pub(crate) source_sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
}

pub(crate) struct PublicSdkCallerSelection {
    pub(crate) repository: String,
    pub(crate) repository_id: u64,
    pub(crate) head_repository: String,
    pub(crate) head_repository_id: u64,
    pub(crate) pull_request_number: u64,
    pub(crate) pull_request_base_sha: String,
    pub(crate) pull_request_head_sha: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
    pub(crate) sdk_source_sha: String,
}

pub(crate) fn verify_public_sdk_release_producer(
    selection: &PublicSdkProducerSelection,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if selection.repository != SDK_REPOSITORY
        || selection.repository_id != SDK_REPOSITORY_ID
        || !is_sha(&selection.source_sha)
        || selection.run_id == 0
        || selection.run_attempt == 0
    {
        return Err(internal("sdk_release_run_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(selection.repository.clone()).with_deadline(deadline);
    api.verify_preview_repository_identity(selection.repository_id, "main")?;
    let workflow = api.admitted_workflow(SDK_WORKFLOW_PATH, SDK_WORKFLOW_NAME)?;
    let run = api.workflow_run(selection.run_id, selection.run_attempt)?;
    let source = RunSource {
        repository_id: selection.repository_id,
        sha: &selection.source_sha,
        branch: "main",
    };
    if !super::run_identity::run_matches(
        &run,
        &workflow,
        selection.run_id,
        selection.run_attempt,
        source,
        "workflow_dispatch",
    ) || run.status.as_deref() != Some("completed")
        || run.conclusion.as_deref() != Some("success")
        || !run_repository_names_match(&run, &selection.repository)
    {
        return Err(internal("sdk_release_run_identity_mismatch"));
    }
    let jobs = api.complete_workflow_jobs(selection.run_id, selection.run_attempt)?;
    require_successful_job(
        &jobs.jobs,
        SDK_ATTEST_MANIFEST_JOB,
        selection.run_id,
        &selection.source_sha,
    )?;
    require_successful_job(
        &jobs.jobs,
        SDK_PUBLISH_JOB,
        selection.run_id,
        &selection.source_sha,
    )?;
    super::verify_public_protected_main_lineage(
        &selection.repository,
        selection.repository_id,
        "main",
        &selection.source_sha,
        &selection.source_sha,
        deadline,
    )?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(())
}

pub(crate) fn verify_public_sdk_adoption_caller(
    selection: &PublicSdkCallerSelection,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(&selection.repository)
        || !velnor_actions_contract::vcs::is_safe_github_repository(&selection.head_repository)
        || selection.repository_id == 0
        || selection.head_repository_id == 0
        || selection.pull_request_number == 0
        || selection.run_id == 0
        || selection.run_attempt == 0
        || !is_sha(&selection.pull_request_base_sha)
        || !is_sha(&selection.pull_request_head_sha)
        || !is_sha(&selection.sdk_source_sha)
    {
        return Err(internal("sdk_adoption_caller_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    if super::read_public_repository_identity(&selection.repository, deadline)?
        != selection.repository_id
    {
        return Err(internal("sdk_adoption_caller_repository_mismatch"));
    }
    let api = GitHubApi::new(selection.repository.clone()).with_deadline(deadline);
    let response: super::github::WorkflowRunWithReferences = api.get_json(&format!(
        "repos/{}/actions/runs/{}/attempts/{}",
        selection.repository, selection.run_id, selection.run_attempt
    ))?;
    validate_adoption_caller_run(&response, selection)?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(())
}

fn validate_adoption_caller_run(
    response: &super::github::WorkflowRunWithReferences,
    selection: &PublicSdkCallerSelection,
) -> Result<(), OrchestratorError> {
    let run = &response.run;
    let referenced_path = format!(
        "tailrocks/velnor-new/{SDK_ADOPTION_CALLEE_PATH}@{}",
        selection.sdk_source_sha
    );
    let expected_references = response
        .referenced_workflows
        .iter()
        .filter(|workflow| {
            workflow.path == referenced_path && workflow.sha == selection.sdk_source_sha
        })
        .count();
    let expected_pull_requests = response
        .pull_requests
        .iter()
        .filter(|pull_request| {
            pull_request.number == selection.pull_request_number
                && pull_request.head.sha == selection.pull_request_head_sha
                && pull_request.base.sha == selection.pull_request_base_sha
                && pull_request.head.ref_name == run.head_branch.as_deref().unwrap_or("")
                && pull_request.head.repo.as_ref().is_some_and(|repo| {
                    pull_request_repo_matches(
                        repo,
                        &selection.head_repository,
                        selection.head_repository_id,
                    )
                })
                && pull_request.base.repo.as_ref().is_some_and(|repo| {
                    pull_request_repo_matches(repo, &selection.repository, selection.repository_id)
                })
        })
        .count();
    if run.id != Some(selection.run_id)
        || run.run_attempt != Some(selection.run_attempt)
        || run.head_sha.as_deref() != Some(selection.pull_request_head_sha.as_str())
        || run.event.as_deref() != Some("pull_request")
        || run.status.as_deref() != Some("in_progress")
        || !run.repository.as_ref().is_some_and(|repo| {
            repo.id == selection.repository_id
                && repo.full_name.as_deref() == Some(selection.repository.as_str())
        })
        || !run.head_repository.as_ref().is_some_and(|repo| {
            repo.id == selection.head_repository_id
                && repo.full_name.as_deref() == Some(selection.head_repository.as_str())
        })
        || response.referenced_workflows.len() != 1
        || expected_references != 1
        || response.pull_requests.len() != 1
        || expected_pull_requests != 1
    {
        return Err(internal("sdk_adoption_caller_identity_mismatch"));
    }
    Ok(())
}

fn pull_request_repo_matches(
    observed: &super::github::WorkflowRunPullRequestRepository,
    expected_repository: &str,
    expected_id: u64,
) -> bool {
    expected_repository
        .rsplit_once('/')
        .is_some_and(|(_, name)| {
            observed.id == expected_id
                && observed.name == name
                && observed.url == format!("https://api.github.com/repos/{expected_repository}")
        })
}

fn run_repository_names_match(run: &super::github::WorkflowRunResponse, repository: &str) -> bool {
    run.repository.as_ref().is_some_and(|repo| {
        repo.id == SDK_REPOSITORY_ID && repo.full_name.as_deref() == Some(repository)
    }) && run.head_repository.as_ref().is_some_and(|repo| {
        repo.id == SDK_REPOSITORY_ID && repo.full_name.as_deref() == Some(repository)
    })
}

fn require_successful_job(
    jobs: &[super::github::WorkflowJob],
    expected_name: &str,
    run_id: u64,
    source_sha: &str,
) -> Result<(), OrchestratorError> {
    let matches = jobs
        .iter()
        .filter(|job| job.name == expected_name)
        .collect::<Vec<_>>();
    if matches.len() != 1
        || matches[0].id == 0
        || matches[0].run_id != run_id
        || matches[0].head_sha.as_deref() != Some(source_sha)
        || matches[0].status.as_deref() != Some("completed")
        || matches[0].conclusion.as_deref() != Some("success")
    {
        return Err(internal("sdk_release_required_job_invalid"));
    }
    Ok(())
}

fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
