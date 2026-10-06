//! Exact tested-candidate and event-base binding for selection.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::GitRequest;

use crate::OrchestratorError;
use crate::internal::internal;
use crate::validators::validate_diff_rev;

/// Resolve the analyzed checkout to its exact integration candidate.
///
/// Identities describe the working tree; a checkout at any other commit
/// would validate the wrong tree. Push and merge-group runs resolve
/// `HEAD` exactly; PR and fork runs additionally accept the merge
/// checkout with exact feature (`HEAD^2`) and event base (`HEAD^1`). Local runs analyze
/// the working tree itself and retain the supplied identity. The caller
/// binds every plan, report, digest, and comparison to the returned SHA.
///
/// # Errors
/// Returns [`OrchestratorError::Internal`] for checkout/head mismatch or
/// unresolvable `HEAD`.
pub(crate) fn verify_checkout(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
) -> Result<String, OrchestratorError> {
    if event == WorkflowEvent::Local {
        return Ok(head.to_owned());
    }
    validate_diff_rev(head, "bad_head").map_err(|problem| internal(&problem))?;
    let checkout = head_sha(root).map_err(|p| internal(&format!("bad_checkout:{p}")))?;
    if checkout == head {
        return Ok(checkout);
    }
    if matches!(event, WorkflowEvent::PullRequest | WorkflowEvent::Fork)
        && checkout_parent(root, "HEAD^2").as_deref() == Some(head)
    {
        if !base.is_some_and(|base| checkout_parent(root, "HEAD^1").as_deref() == Some(base)) {
            return Err(internal("checkout_base_mismatch"));
        }
        return Ok(checkout);
    }
    Err(internal("checkout_head_mismatch"))
}

/// Exact parent of the checkout merge commit, if present.
fn checkout_parent(root: &Path, parent: &str) -> Option<String> {
    let output = GitRequest::rev_parse(vec![OsString::from(parent)])
        .run_in(root)
        .ok()?;
    if !output.success {
        return None;
    }
    let sha = output.stdout_text("git").ok()?.trim().to_owned();
    validate_diff_rev(&sha, "bad_head").ok()?;
    Some(sha)
}

/// Resolve `HEAD` to a SHA for local comparison.
pub(super) fn head_sha(root: &Path) -> Result<String, String> {
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    if !output.success {
        return Err("missing_head".to_owned());
    }
    let sha = output
        .stdout_text("git")
        .map_err(|err| err.to_string())?
        .trim()
        .to_owned();
    validate_diff_rev(&sha, "bad_head")?;
    Ok(sha)
}
