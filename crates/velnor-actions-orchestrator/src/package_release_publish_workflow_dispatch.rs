use velnor_actions_contract::{CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME};
use velnor_actions_mise::CheckDeadline;

use super::{GitHubApi, WorkflowRunResponse, github};
use crate::{OrchestratorError, internal::internal};

#[derive(Clone)]
pub(crate) struct QualificationRunSelection {
    repository: String,
    repository_id: u64,
    source_sha: String,
    workflow_sha: String,
    workflow_ref: String,
    run_id: u64,
    run_attempt: u32,
}

impl QualificationRunSelection {
    pub(crate) fn new(
        repository: String,
        repository_id: u64,
        source_sha: String,
        workflow_sha: String,
        workflow_ref: String,
        run_id: u64,
        run_attempt: u32,
    ) -> Self {
        Self {
            repository,
            repository_id,
            source_sha,
            workflow_sha,
            workflow_ref,
            run_id,
            run_attempt,
        }
    }
}

pub(crate) fn verify_public_qualification_run(
    selection: &QualificationRunSelection,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(&selection.repository)
        || selection.repository_id == 0
        || !github::is_lowercase_git_sha(&selection.source_sha)
        || selection.workflow_sha != selection.source_sha
        || selection.workflow_ref
            != format!(
                "{}/{CI_WORKFLOW_PATH}@refs/heads/main",
                selection.repository
            )
        || selection.run_id == 0
        || selection.run_attempt == 0
    {
        return Err(internal("package_public_qualification_selection_invalid"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(selection.repository.clone()).with_deadline(deadline);
    api.verify_preview_repository_identity(selection.repository_id, "main")?;
    let workflow = api.admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
    let run = api.workflow_run(selection.run_id, selection.run_attempt)?;
    validate_run(&run, workflow.id, selection)?;
    let protected_tip = super::verify_public_protected_main_lineage(
        &selection.repository,
        selection.repository_id,
        "main",
        &selection.source_sha,
        &selection.workflow_sha,
        deadline,
    )?;
    if protected_tip != selection.source_sha {
        return Err(internal("package_public_qualification_main_moved"));
    }
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(())
}

fn validate_run(
    run: &WorkflowRunResponse,
    workflow_id: u64,
    selection: &QualificationRunSelection,
) -> Result<(), OrchestratorError> {
    let same_repo = |repo: &Option<github::RunRepository>| {
        repo.as_ref().is_some_and(|observed| {
            observed.id == selection.repository_id
                && observed.full_name.as_deref() == Some(selection.repository.as_str())
        })
    };
    if run.id != Some(selection.run_id)
        || run.run_attempt != Some(selection.run_attempt)
        || run.workflow_id != Some(workflow_id)
        || run.path.as_deref() != Some(CI_WORKFLOW_PATH)
        || run.event.as_deref() != Some("workflow_dispatch")
        || run.status.as_deref() != Some("in_progress")
        || run.conclusion.is_some()
        || run.head_sha.as_deref() != Some(selection.source_sha.as_str())
        || run.head_branch.as_deref() != Some("main")
        || !same_repo(&run.repository)
        || !same_repo(&run.head_repository)
    {
        return Err(internal("package_public_qualification_run_identity"));
    }
    Ok(())
}
