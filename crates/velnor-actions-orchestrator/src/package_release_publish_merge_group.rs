use std::collections::BTreeMap;
use velnor_actions_contract::{CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

use super::{GitHubApi, WorkflowMetadata, github, github::WorkflowRunResponse};

#[derive(Clone)]
pub(crate) struct MergeGroupSelection {
    repository: String,
    repository_id: u64,
    base_sha: String,
    head_sha: String,
    head_ref: String,
    head_branch: String,
    run_id: u64,
    run_attempt: u32,
}

pub(crate) struct MergeGroupFacts {
    pub(crate) repository: String,
    pub(crate) repository_id: u64,
    pub(crate) base_sha: String,
    pub(crate) head_sha: String,
    pub(crate) head_ref: String,
    pub(crate) head_branch: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u32,
}

impl MergeGroupSelection {
    pub(crate) fn new(facts: MergeGroupFacts) -> Self {
        Self {
            repository: facts.repository,
            repository_id: facts.repository_id,
            base_sha: facts.base_sha,
            head_sha: facts.head_sha,
            head_ref: facts.head_ref,
            head_branch: facts.head_branch,
            run_id: facts.run_id,
            run_attempt: facts.run_attempt,
        }
    }

    pub(crate) fn repository(&self) -> &str {
        &self.repository
    }
    pub(crate) const fn repository_id(&self) -> u64 {
        self.repository_id
    }
    pub(crate) fn base_sha(&self) -> &str {
        &self.base_sha
    }
    pub(crate) fn head_sha(&self) -> &str {
        &self.head_sha
    }
    pub(crate) fn head_ref(&self) -> &str {
        &self.head_ref
    }
    pub(crate) fn head_branch(&self) -> &str {
        &self.head_branch
    }
    pub(crate) const fn run_id(&self) -> u64 {
        self.run_id
    }
    pub(crate) const fn run_attempt(&self) -> u32 {
        self.run_attempt
    }
}

pub(crate) fn verify_public_merge_group_context(
    selection: &MergeGroupSelection,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let api = GitHubApi::new(selection.repository().to_owned()).with_deadline(deadline);
    verify_with_api(&api, selection)?;
    deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    Ok(())
}

fn verify_with_api(
    api: &GitHubApi,
    selection: &MergeGroupSelection,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::vcs::is_safe_github_repository(selection.repository())
        || selection.repository_id() == 0
        || !github::is_lowercase_git_sha(selection.base_sha())
        || !github::is_lowercase_git_sha(selection.head_sha())
        || selection.run_id() == 0
        || selection.run_attempt() == 0
        || selection.head_ref().strip_prefix("refs/heads/") != Some(selection.head_branch())
        || !valid_queue_branch(selection.head_branch())
    {
        return Err(internal("package_public_merge_group_selection_invalid"));
    }
    api.verify_preview_repository_identity(selection.repository_id(), "main")?;
    let workflow = api.admitted_workflow(CI_WORKFLOW_PATH, WORKFLOW_DISPLAY_NAME)?;
    let run = api.workflow_run(selection.run_id(), selection.run_attempt())?;
    validate_run(&run, &workflow, selection)?;
    verify_queue_ref(api, selection)?;
    github::verify_public_pull_request_lineage(
        api,
        selection.repository_id(),
        "main",
        selection.base_sha(),
        selection.head_sha(),
    )?;
    Ok(())
}

fn valid_queue_branch(branch: &str) -> bool {
    velnor_actions_contract::vcs::VcsInputs {
        commit: None,
        reference: Some(format!("refs/heads/{branch}")),
        submodules: BTreeMap::default(),
    }
    .validate()
    .is_ok()
}

fn validate_run(
    run: &WorkflowRunResponse,
    workflow: &WorkflowMetadata,
    selection: &MergeGroupSelection,
) -> Result<(), OrchestratorError> {
    let same_repo = |repo: &Option<github::RunRepository>| {
        repo.as_ref().is_some_and(|observed| {
            observed.id == selection.repository_id()
                && observed.full_name.as_deref() == Some(selection.repository())
        })
    };
    if run.id != Some(selection.run_id())
        || run.run_attempt != Some(selection.run_attempt())
        || run.event.as_deref() != Some("merge_group")
        || run.status.as_deref() != Some("in_progress")
        || run.conclusion.is_some()
        || run.head_sha.as_deref() != Some(selection.head_sha())
        || run.head_branch.as_deref() != Some(selection.head_branch())
        || run.path.as_deref() != Some(CI_WORKFLOW_PATH)
        || run.workflow_id != Some(workflow.id)
        || !same_repo(&run.repository)
        || !same_repo(&run.head_repository)
    {
        return Err(internal("package_public_merge_group_run_identity"));
    }
    Ok(())
}

fn verify_queue_ref(
    api: &GitHubApi,
    selection: &MergeGroupSelection,
) -> Result<(), OrchestratorError> {
    let references: Vec<github::GitRefResponse> = api.get_json(&api.route(&format!(
        "git/matching-refs/heads/{}",
        github::encode_segment(selection.head_branch())
    )))?;
    let expected_ref = selection.head_ref();
    let matches = references
        .iter()
        .filter(|reference| reference.ref_name == expected_ref)
        .collect::<Vec<_>>();
    if matches.len() != 1
        || matches[0].object.object_type != "commit"
        || !github::is_lowercase_git_sha(&matches[0].object.sha)
        || matches[0].object.sha != selection.head_sha()
    {
        return Err(internal("package_public_merge_group_ref_mismatch"));
    }
    Ok(())
}
