//! Output-path and context-scalar guards: traversal rejection, symlink
//! probing, plus the pinned label/binary/directory shapes every render
//! context carries.
//!
//! Pure helpers: lexical checks run here, symlink detection uses a
//! caller-supplied probe so this crate never touches the filesystem.

use std::path::{Path, PathBuf};

use velnor_actions_workflow_steps::{RenderError, steps};

/// A validated relative output path inside the generated tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeTreePath(String);

impl SafeTreePath {
    /// Borrow the validated path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Reject absolute paths, traversal, backslashes, and control characters.
///
/// # Errors
///
/// Returns [`RenderError::UnsafePath`] for empty, overlong, absolute,
/// traversing, or otherwise unsafe paths.
pub fn validate_tree_path(path: &str) -> Result<SafeTreePath, RenderError> {
    if path.is_empty() || path.len() > 4096 {
        return Err(RenderError::UnsafePath(format!("bad_length:{path}")));
    }
    if path.starts_with('/') || path.contains('\\') || path.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(RenderError::UnsafePath(format!("bad_chars:{path}")));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(RenderError::UnsafePath(format!("bad_segment:{path}")));
        }
    }
    Ok(SafeTreePath(path.to_owned()))
}

/// Validate a generated-tree path against a fixed allowlist.
///
/// Lexical validation first, then exact allowlist membership: release
/// files cannot smuggle arbitrary paths into the tree.
///
/// # Errors
///
/// Returns [`RenderError::UnsafePath`] for unsafe or unlisted paths.
pub fn validate_allowlisted_path(
    path: &str,
    allowed: &[&str],
) -> Result<SafeTreePath, RenderError> {
    let safe = validate_tree_path(path)?;
    if allowed.contains(&path) {
        Ok(safe)
    } else {
        Err(RenderError::UnsafePath(format!("unlisted_path:{path}")))
    }
}

/// Require a literal versioned Ubuntu label (no aliases or expressions).
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for unpinned labels.
pub fn validate_runs_on(label: &str) -> Result<(), RenderError> {
    let pinned = !label.is_empty()
        && label.starts_with("ubuntu-")
        && !label.contains("${{")
        && !label.contains("latest")
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'));
    if pinned {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "unpinned_label:{label}"
        )))
    }
}

/// Require the staged path with the exact generator version suffix.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for unstaged binaries.
pub fn validate_staged_binary(staged: &str, version: &str) -> Result<(), RenderError> {
    match staged.strip_prefix(steps::STAGED_BINARY_PREFIX) {
        Some(suffix) if suffix == version => Ok(()),
        _ => Err(RenderError::InvalidWorkflow(format!(
            "unstaged_binary:{staged}"
        ))),
    }
}

/// Require a runner-temp request directory without traversal.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for malformed directories.
pub fn validate_request_dir(dir: &str) -> Result<(), RenderError> {
    match dir.strip_prefix(steps::REQUEST_DIR_PREFIX) {
        Some(rest)
            if !rest.is_empty()
                && !rest.split('/').any(|seg| seg.is_empty() || seg == "..")
                && !rest.chars().any(|ch| ch.is_whitespace() || ch.is_control()) =>
        {
            Ok(())
        }
        _ => Err(RenderError::InvalidWorkflow(format!(
            "bad_request_dir:{dir}"
        ))),
    }
}

/// Lexically join a safe path under `root` (cannot escape by construction).
#[must_use]
pub fn join_within_root(root: &Path, rel: &SafeTreePath) -> PathBuf {
    root.join(&rel.0)
}

/// Fail when any generated-tree prefix is a symlink (caller probe).
///
/// Walks `root/<segment...>` prefixes and calls `is_symlink` on each one.
/// The crate performs no filesystem IO itself.
///
/// # Errors
///
/// Returns [`RenderError::UnsafePath`] naming the symlinked prefix.
pub fn check_no_symlink<F>(
    root: &Path,
    rel: &SafeTreePath,
    is_symlink: F,
) -> Result<(), RenderError>
where
    F: Fn(&Path) -> bool,
{
    let mut prefix = PathBuf::from(root);
    for segment in rel.0.split('/') {
        prefix.push(segment);
        if is_symlink(&prefix) {
            return Err(RenderError::UnsafePath(format!(
                "symlink_prefix:{}",
                prefix.display()
            )));
        }
    }
    Ok(())
}
