//! Exact top-level workflow policy checks shared by render entrypoints.

use velnor_actions_contract::{Concurrency, Trigger, WorkflowIr};

use crate::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, EXPECTED_PR_TYPES, RenderError};

/// Require the exact trigger shape: 4 PR types, one push branch, merge group.
pub(crate) fn check_triggers(triggers: &Trigger) -> Result<(), RenderError> {
    let expected: Vec<String> = EXPECTED_PR_TYPES.iter().map(ToString::to_string).collect();
    if triggers.pull_request_types != expected {
        return Err(RenderError::InvalidWorkflow("bad_pr_triggers".to_owned()));
    }
    let branch_ok = triggers.push_branches.len() == 1
        && triggers.push_branches.first().is_some_and(|branch| {
            !branch.trim().is_empty() && !branch.chars().any(char::is_whitespace)
        });
    if !branch_ok {
        return Err(RenderError::InvalidWorkflow("bad_push_branch".to_owned()));
    }
    if !triggers.merge_group {
        return Err(RenderError::InvalidWorkflow(
            "missing_merge_group".to_owned(),
        ));
    }
    Ok(())
}

/// Require the exact concurrency group plus PR-only cancel.
pub(crate) fn check_concurrency(concurrency: &Concurrency) -> Result<(), RenderError> {
    if concurrency.group != CONCURRENCY_GROUP
        || concurrency.cancel_in_progress != CONCURRENCY_CANCEL
    {
        return Err(RenderError::InvalidWorkflow("bad_concurrency".to_owned()));
    }
    Ok(())
}

/// Require every job to use the single context label.
pub(crate) fn check_single_label(ir: &WorkflowIr, label: &str) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        if job.runs_on != label
            && !velnor_actions_contract::RunsOn::parse(&job.runs_on)
                .is_ok_and(|selector| selector.is_scale_set())
        {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    Ok(())
}
