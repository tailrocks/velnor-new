//! Shared git-origin resolution through Git's config query.
//!
//! The single `remote.origin.url` reader for identity checks: both the
//! Velnor-repository policy (`prepare`) and the baseline provenance
//! anchor (`provenance_check`) resolve through here, so linked
//! worktrees (`.git` is a file), includes, and worktree configuration
//! all follow Git semantics. No caller hand-parses `.git/config`.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

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
