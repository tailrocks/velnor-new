//! Captured immutable runner and API context for protected-main qualification.

use serde_json::Value;
use std::path::Path;
use velnor_actions_contract::QualificationDispatch;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[derive(Clone)]
pub(super) struct QualificationSelection {
    dispatch: QualificationDispatch,
    repository_id: u64,
    source_sha: String,
}

impl QualificationSelection {
    pub(super) fn source_sha(&self) -> &str {
        &self.source_sha
    }
}

pub(super) fn authenticate(
    repository: &str,
    repository_id: u64,
    expected_head: &str,
    checkout_head: &str,
    deadline: CheckDeadline,
) -> Result<QualificationSelection, OrchestratorError> {
    let dispatch = capture_dispatch(repository, expected_head)?;
    if checkout_head != expected_head || dispatch.repository != repository {
        return Err(internal("homebrew_authority_qualification_checkout"));
    }
    let selection = public_selection(&dispatch, repository_id);
    crate::package_release_publish::verify_public_qualification_run(&selection, deadline)?;
    Ok(QualificationSelection {
        dispatch,
        repository_id,
        source_sha: expected_head.to_owned(),
    })
}

pub(super) fn recheck(
    repository: &str,
    repository_id: u64,
    expected_head: &str,
    selection: &QualificationSelection,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if repository_id != selection.repository_id || expected_head != selection.source_sha {
        return Err(internal(
            "homebrew_authority_qualification_selection_changed",
        ));
    }
    let observed = capture_dispatch(repository, expected_head)?;
    if observed != selection.dispatch {
        return Err(internal("homebrew_authority_qualification_runner_changed"));
    }
    crate::package_release_publish::verify_public_qualification_run(
        &public_selection(&observed, repository_id),
        deadline,
    )
}

fn capture_dispatch(
    repository: &str,
    expected_head: &str,
) -> Result<QualificationDispatch, OrchestratorError> {
    let event_name = std::env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    let event_path = std::env::var_os("GITHUB_EVENT_PATH")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("homebrew_authority_qualification_event_missing"))?;
    let bytes = crate::safe_read::read_event_file(
        Path::new(&event_path),
        crate::safe_read::MAX_REPO_FILE_BYTES,
    )?;
    let payload: Value = serde_json::from_str(&bytes)
        .map_err(|_| internal("homebrew_authority_qualification_event_invalid"))?;
    let github_repository = std::env::var("GITHUB_REPOSITORY").ok();
    let git_ref = std::env::var("GITHUB_REF").ok();
    let ref_protected = std::env::var("GITHUB_REF_PROTECTED").ok();
    let workflow_ref = std::env::var("GITHUB_WORKFLOW_REF").ok();
    let workflow_sha = std::env::var("GITHUB_WORKFLOW_SHA").ok();
    let source_sha = std::env::var("GITHUB_SHA").ok();
    let run_id = std::env::var("GITHUB_RUN_ID").ok();
    let run_attempt = std::env::var("GITHUB_RUN_ATTEMPT").ok();
    let runner = crate::request_event::QualificationRunnerContext {
        repository: github_repository.as_deref(),
        git_ref: git_ref.as_deref(),
        ref_protected: ref_protected.as_deref(),
        workflow_ref: workflow_ref.as_deref(),
        workflow_sha: workflow_sha.as_deref(),
        source_sha: source_sha.as_deref(),
        run_id: run_id.as_deref(),
        run_attempt: run_attempt.as_deref(),
    };
    let dispatch =
        crate::request_event::qualification_dispatch_for_parts(&event_name, &payload, runner)?
            .ok_or_else(|| internal("homebrew_authority_qualification_event"))?;
    if !dispatch_is_admitted(&dispatch, repository, expected_head, &event_name) {
        return Err(internal("homebrew_authority_qualification_runner_context"));
    }
    Ok(dispatch)
}

fn dispatch_is_admitted(
    dispatch: &QualificationDispatch,
    repository: &str,
    expected_head: &str,
    event_name: &str,
) -> bool {
    event_name == "workflow_dispatch"
        && dispatch.repository == repository
        && dispatch.source_sha == expected_head
        && dispatch.workflow_sha == expected_head
        && dispatch.git_ref == "refs/heads/main"
        && dispatch.default_branch == "main"
        && dispatch
            .validate_for("main", repository, expected_head)
            .is_ok()
}

fn public_selection(
    dispatch: &QualificationDispatch,
    repository_id: u64,
) -> crate::package_release_publish::QualificationRunSelection {
    crate::package_release_publish::QualificationRunSelection::new(
        dispatch.repository.clone(),
        repository_id,
        dispatch.source_sha.clone(),
        dispatch.workflow_sha.clone(),
        dispatch.workflow_ref.clone(),
        dispatch.run_id,
        dispatch.run_attempt,
    )
}

#[cfg(test)]
#[path = "qualification_context_tests.rs"]
mod tests;
