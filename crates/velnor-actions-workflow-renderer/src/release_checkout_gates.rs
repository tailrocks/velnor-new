//! Anonymous source checkouts pin approved identity and discard credentials.
use crate::{RenderError, release_jobs::ReleaseRole, release_tree::RELEASE_SOURCE_DIR};
use velnor_actions_contract::{Step, StepKind};
/// Validate checkout repository, ref, history, path, and credential shape.
/// # Errors
/// Rejects checkout identity, history, path, or credential violations.
pub fn check_checkout_shape(
    id: &str,
    steps: &[Step],
    _role: ReleaseRole,
    sha: &str,
    repository: &str,
) -> Result<(), RenderError> {
    for step in steps {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            continue;
        };
        if !uses.starts_with("actions/checkout@") {
            continue;
        }
        if with
            .get("repository")
            .is_some_and(|value| value != repository)
        {
            return invalid(id, "foreign_checkout_repository");
        }
        if with
            .get("persist-credentials")
            .is_none_or(|value| value != "false")
        {
            return invalid(id, "checkout_with_credentials");
        }
        if with.get("fetch-depth").is_none_or(|value| value != "0") {
            return invalid(id, "shallow_checkout");
        }
        if with
            .get("path")
            .is_none_or(|value| value != RELEASE_SOURCE_DIR)
        {
            return invalid(id, "unknown_checkout_path");
        }
        if with.get("ref").is_none_or(|value| value != sha) {
            return invalid(id, "source_ref_mismatch");
        }
    }
    Ok(())
}
fn has_exact_checkout(steps: &[Step], sha: &str, repository: &str) -> bool {
    steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, with, .. }
            if uses.starts_with("actions/checkout@")
                && with
                    .get("repository")
                    .is_none_or(|value| value == repository)
                && with.get("path").is_some_and(|value| value == RELEASE_SOURCE_DIR)
                && with.get("ref").is_some_and(|value| value == sha))
    })
}
/// Require an exact approved source checkout.
/// # Errors
/// Returns an error when no checkout matches the approved repository and ref.
pub fn require_exact_checkout(
    id: &str,
    steps: &[Step],
    sha: &str,
    repository: &str,
) -> Result<(), RenderError> {
    if has_exact_checkout(steps, sha, repository) {
        Ok(())
    } else {
        invalid(id, "checkout_without_exact_source")
    }
}
fn invalid<T>(id: &str, code: &str) -> Result<T, RenderError> {
    Err(RenderError::InvalidWorkflow(format!("{code}:{id}")))
}
