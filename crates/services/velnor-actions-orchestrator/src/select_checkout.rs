//! Checkout identity verification, including deadline-bound named checks.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::{CheckDeadline, GitRequest};

use crate::OrchestratorError;
use crate::internal::internal;
use crate::validators::validate_diff_rev;

/// Verify that the analyzed checkout matches the intended head.
///
/// Push and merge-group runs resolve `HEAD` exactly; PR and fork runs also
/// accept the merge checkout's second parent. Local runs use the worktree.
pub(crate) fn verify_checkout(
    root: &Path,
    event: WorkflowEvent,
    head: &str,
) -> Result<(), OrchestratorError> {
    verify_checkout_inner(root, event, head, None)
}

/// Verify checkout identity under the named check's shared absolute deadline.
pub(crate) fn verify_checkout_until(
    root: &Path,
    event: WorkflowEvent,
    head: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    verify_checkout_inner(root, event, head, Some(deadline))
}

fn verify_checkout_inner(
    root: &Path,
    event: WorkflowEvent,
    head: &str,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    if event == WorkflowEvent::Local {
        return Ok(());
    }
    validate_diff_rev(head, "bad_head").map_err(|problem| internal(&problem))?;
    let checkout =
        head_sha(root, deadline).map_err(|problem| internal(&format!("bad_checkout:{problem}")))?;
    if checkout == head {
        return Ok(());
    }
    if matches!(event, WorkflowEvent::PullRequest | WorkflowEvent::Fork)
        && second_parent(root, deadline)
            .map_err(|problem| internal(&format!("bad_checkout:{problem}")))?
            .as_deref()
            == Some(head)
    {
        return Ok(());
    }
    Err(internal("checkout_head_mismatch"))
}

/// Resolve `HEAD` under an optional caller-owned deadline.
fn head_sha(root: &Path, deadline: Option<CheckDeadline>) -> Result<String, String> {
    let request = GitRequest::rev_parse(vec![OsString::from("HEAD")]);
    let output = run_git(&request, root, deadline)?;
    if !output.success {
        return Err("missing_head".to_owned());
    }
    let sha = output
        .stdout_text("git")
        .map_err(|error| error.to_string())?
        .trim()
        .to_owned();
    validate_diff_rev(&sha, "bad_head")?;
    Ok(sha)
}

/// Second parent of a merge checkout, if present.
fn second_parent(root: &Path, deadline: Option<CheckDeadline>) -> Result<Option<String>, String> {
    let request = GitRequest::rev_parse(vec![OsString::from("HEAD^2")]);
    let output = run_git(&request, root, deadline)?;
    if !output.success {
        return Ok(None);
    }
    let Ok(sha) = output.stdout_text("git") else {
        return Ok(None);
    };
    let sha = sha.trim().to_owned();
    if validate_diff_rev(&sha, "bad_head").is_err() {
        return Ok(None);
    }
    Ok(Some(sha))
}

fn run_git(
    request: &GitRequest,
    root: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<velnor_actions_mise::ProcessOutput, String> {
    match deadline {
        Some(deadline) => request.run_in_until(root, deadline),
        None => request.run_in(root),
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::verify_checkout_until;
    use std::time::Duration;
    use velnor_actions_contract::WorkflowEvent;
    use velnor_actions_mise::CheckDeadline;

    #[test]
    fn checkout_git_must_fit_the_shared_deadline() {
        let root = tempfile::tempdir().expect("checkout root");
        let started = std::time::Instant::now()
            .checked_sub(Duration::from_millis(20))
            .expect("past start");
        let deadline =
            CheckDeadline::from_start(started, Duration::from_millis(10)).expect("deadline");
        let error = verify_checkout_until(
            root.path(),
            WorkflowEvent::Push,
            "0123456789abcdef0123456789abcdef01234567",
            deadline,
        )
        .expect_err("expired Git checkout verification");
        assert!(
            error
                .to_string()
                .contains("timeout_after_absolute_deadline"),
            "checkout reports its expired shared deadline: {error}"
        );
    }
}
