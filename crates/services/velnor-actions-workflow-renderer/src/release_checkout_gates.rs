//! Release checkout gates: policy-plus-source shape and credentials.
//!
//! Policy checkouts (no `path`) track the event commit and carry no `ref`
//! pin; source checkouts (fixed `path`) pin the approved SHA. Every
//! checkout fetches full history, and only checkouts that must push
//! (preparation's policy checkout, publishers' source checkouts) persist
//! credentials.

use velnor_actions_contract_workflow::{Step, StepKind};

use crate::{release_jobs::ReleaseRole, release_tree::RELEASE_SOURCE_DIR};
use velnor_actions_workflow_steps::RenderError;

/// Expected `persist-credentials` for one checkout kind.
///
/// Only checkouts that must push carry credentials: preparation pushes
/// the release-pr branch, publishers push tags. Everything else stays
/// credential-free.
fn expected_persist(role: ReleaseRole, sourced: bool) -> &'static str {
    let pushes = (!sourced && role == ReleaseRole::Preparation)
        || (sourced
            && matches!(
                role,
                ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap
            ));
    if pushes { "true" } else { "false" }
}

/// Validate the shape of every release checkout step.
///
/// Policy checkouts track the event commit (release-pr requires a
/// branch, and a pinned policy would desync the workflow from its
/// configs); source checkouts pin the approved SHA under the fixed
/// path. Every checkout fetches full history: release-plz needs tags
/// plus history for selection and changelogs.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for wrong credentials,
/// shallow history, pinned policies, stray paths, or rebound refs.
pub fn check_checkout_shape(
    id: &str,
    steps: &[Step],
    role: ReleaseRole,
    sha: &str,
) -> Result<(), RenderError> {
    for step in steps {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            continue;
        };
        if !uses.starts_with("actions/checkout@") {
            continue;
        }
        let sourced = with.contains_key("path");
        if with
            .get("persist-credentials")
            .is_none_or(|value| value != expected_persist(role, sourced))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "checkout_with_credentials:{id}"
            )));
        }
        if with.get("fetch-depth").is_none_or(|value| value != "0") {
            return Err(RenderError::InvalidWorkflow(format!(
                "shallow_checkout:{id}"
            )));
        }
        if !sourced {
            if with.contains_key("ref") {
                return Err(RenderError::InvalidWorkflow(format!(
                    "pinned_policy_checkout:{id}"
                )));
            }
            continue;
        }
        if with
            .get("path")
            .is_none_or(|value| value != RELEASE_SOURCE_DIR)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "unknown_checkout_path:{id}"
            )));
        }
        if with.get("ref").is_none_or(|value| value != sha) {
            return Err(RenderError::InvalidWorkflow(format!(
                "source_ref_mismatch:{id}"
            )));
        }
    }
    Ok(())
}

/// True when a source checkout pins `path` plus `ref` to the approved source.
fn has_exact_checkout(steps: &[Step], sha: &str) -> bool {
    steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, with, .. }
            if uses.starts_with("actions/checkout@")
                && with.get("path").is_some_and(|value| value == RELEASE_SOURCE_DIR)
                && with.get("ref").is_some_and(|value| value == sha))
    })
}

/// Require an exact-source checkout (no fallback to unverified checkouts).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] when no checkout pins the
/// approved source path plus SHA.
pub fn require_exact_checkout(id: &str, steps: &[Step], sha: &str) -> Result<(), RenderError> {
    if has_exact_checkout(steps, sha) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "checkout_without_exact_source:{id}"
        )))
    }
}
