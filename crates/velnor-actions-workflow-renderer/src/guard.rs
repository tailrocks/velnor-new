//! Output-path guards: lexical traversal rejection plus symlink probing.
//!
//! Pure helpers: lexical checks run here, symlink detection uses a
//! caller-supplied probe so this crate never touches the filesystem.

use std::path::{Path, PathBuf};

use crate::RenderError;

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
