//! Shared git-origin resolution through Git's config query.
//!
//! The single `remote.origin.url` reader for identity checks: both the
//! Velnor-repository policy (`prepare`) and the baseline provenance
//! anchor (`provenance_check`) resolve through here, so linked
//! worktrees (`.git` is a file), includes, and worktree configuration
//! all follow Git semantics. No caller hand-parses `.git/config`.
//!
//! The git origin is mutable: any step can rewrite it before the plan
//! or merge runs. Repository expectations therefore prefer the
//! runner-owned [`GITHUB_REPOSITORY_ENV`] slug and treat the origin as
//! a local-run fallback only; provenance resolution reports an
//! origin/env disagreement as a conflict the caller fails closed on.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

/// Runner-owned repository slug variable (`owner/repo`).
///
/// The runner sets it immutably per job; workflow steps cannot change
/// it, so it anchors repository expectations ahead of the git origin.
pub(crate) const GITHUB_REPOSITORY_ENV: &str = "GITHUB_REPOSITORY";

/// Origin URL via `git config --get remote.origin.url` in `root`.
///
/// Returns `None` when Git cannot run, exits nonzero (no such key),
/// prints non-text, or prints a blank value; every miss fails closed
/// at the caller. Nothing is fetched.
pub(crate) fn origin_url_via_git(root: &Path) -> Option<String> {
    let output = GitRequest::config(vec![
        OsString::from("--get"),
        OsString::from("remote.origin.url"),
    ])
    .run_in(root)
    .ok()?;
    if !output.success {
        return None;
    }
    let url = output.stdout_text("git").ok()?;
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Lowercase `owner/repo` slug from raw text, when well-shaped.
///
/// Exactly two nonempty segments around one `/`, no whitespace; hosting
/// slugs compare case-insensitively, so the result lowercases. Anything
/// else is not a slug: callers fall back or fail closed, never guess.
pub(crate) fn validate_repository_slug(raw: &str) -> Option<String> {
    if raw.chars().any(char::is_whitespace) {
        return None;
    }
    let mut parts = raw.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{}/{}", owner.to_lowercase(), repo.to_lowercase()))
}
#[cfg(test)]
mod tests;
