//! Publisher lock and release-event eligibility.
//!
//! Declared from `release_spec.rs` (`#[path]`, no `lib.rs` edit); that
//! module re-exports the lock types so `release_spec::X` paths keep
//! working. New helpers stay under `release_spec::lock::`.

use super::{is_clean_text, validate_repository};
use crate::{RenderError, steps::scan_for_private_subcommands};

/// Stable serialized lock: fixed key, publishers never cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseConcurrency {
    /// Lock group expression (stable per repository/workspace).
    pub group: String,
    /// Must always be false: never cancel an active publisher.
    pub cancel_in_progress: bool,
}

/// Tokens that would make the lock run-, version-, or source-unique.
const FORBIDDEN_LOCK_TOKENS: &[&str] = &[
    "run_id",
    "run_attempt",
    "run_number",
    "github.sha",
    "github.ref",
    "github.event",
    "inputs.",
    "matrix.",
    "version",
    "strategy",
];

impl ReleaseConcurrency {
    /// Validate the stable group plus the never-cancel invariant.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidWorkflow`] or
    /// [`RenderError::PrivateSubcommand`] for unstable groups or cancel.
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.group, 256) {
            return Err(RenderError::InvalidWorkflow(format!(
                "bad_lock_group:{}",
                self.group
            )));
        }
        for token in FORBIDDEN_LOCK_TOKENS {
            if self.group.contains(token) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "forbidden_lock_token:{token}"
                )));
            }
        }
        scan_for_private_subcommands(&self.group)?;
        if self.cancel_in_progress {
            return Err(RenderError::InvalidWorkflow("publisher_cancel".to_owned()));
        }
        Ok(())
    }
}

/// Require the lock key to anchor on the repository identity.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for unanchored groups.
pub fn check_lock_anchor(group: &str, repository: &str) -> Result<(), RenderError> {
    if group.contains("github.repository") || group.contains(repository) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "unstable_lock_anchor".to_owned(),
        ))
    }
}

/// Derive the stable publisher lock for one registry/repository/workspace.
///
/// Key shape is literal (no expressions): `release-<registry>-<repository>-<workspace>`.
/// It is stable across runs, versions, and sources, anchored on the
/// repository identity, and never cancels an active publisher.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed segments.
pub fn stable_lock_group(
    registry: &str,
    repository: &str,
    workspace: &str,
) -> Result<ReleaseConcurrency, RenderError> {
    validate_lock_segment(registry, "registry")?;
    validate_repository(repository)?;
    validate_lock_segment(workspace, "workspace")?;
    Ok(ReleaseConcurrency {
        group: format!("release-{registry}-{repository}-{workspace}"),
        cancel_in_progress: false,
    })
}

/// Validate one lock segment (registry or workspace): lowercase shape.
fn validate_lock_segment(segment: &str, kind: &str) -> Result<(), RenderError> {
    let shaped = !segment.is_empty()
        && segment.len() <= 64
        && segment
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'));
    if shaped {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bad_lock_{kind}:{segment}"
        )))
    }
}

/// GitHub events eligible to run release work.
const RELEASE_ELIGIBLE_EVENTS: &[&str] = &["push", "schedule", "workflow_dispatch"];

/// Reject release-ineligible events: pull requests, forks, and runners.
///
/// `pull_request_target` and `workflow_run` never publish: the first
/// executes untrusted PR content with base privileges, the second
/// replays another workflow's context. Fork runs fail the repository
/// gate as well; this rejects them by event name before any dispatch.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for ineligible events.
pub fn reject_prohibited_release_event(event: &str) -> Result<(), RenderError> {
    if RELEASE_ELIGIBLE_EVENTS.contains(&event) {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "prohibited_release_event:{event}"
        )))
    }
}
