//! Symlink-prefix guard with emitted-leaf replacement.
//!
//! Staging recreates every emitted leaf fresh, so a legacy symlink exactly
//! at an emitted leaf cannot divert a write; the atomic swap drops it.
//! Symlink ancestors still fail closed: the preserved copy walks live
//! ancestor directories.

use std::path::Path;

use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};

use crate::OrchestratorError;

/// Probe for [`guard::check_no_symlink`]: true when the prefix is a symlink.
pub(crate) fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_symlink())
}

/// Refuse symlink prefixes, except a symlink exactly at an emitted leaf.
pub(crate) fn check_no_symlink_or_emitted_leaf(
    root: &Path,
    rel: &SafeTreePath,
) -> Result<(), OrchestratorError> {
    match guard::check_no_symlink(root, rel, is_symlink) {
        Ok(()) => Ok(()),
        Err(err) => {
            if emitted_leaf_symlink_only(root, rel) {
                Ok(())
            } else {
                Err(err.into())
            }
        }
    }
}

/// True when the leaf is a symlink and every strict ancestor is clean.
fn emitted_leaf_symlink_only(root: &Path, rel: &SafeTreePath) -> bool {
    let mut segments = rel.as_str().split('/');
    let Some(leaf) = segments.next_back() else {
        return false;
    };
    let mut prefix = root.to_path_buf();
    for segment in segments {
        prefix.push(segment);
        if is_symlink(&prefix) {
            return false;
        }
    }
    prefix.push(leaf);
    is_symlink(&prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> std::io::Result<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("velnor-leaf-{test}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn emitted_leaf_symlink_passes_but_ancestor_fails() {
        let root = scratch("leaf_link").expect("scratch");
        let github = root.join(".github");
        std::fs::create_dir_all(github.join("workflows")).expect("dirs");
        std::os::unix::fs::symlink("AGENTS.md", github.join("CLAUDE.md")).expect("leaf link");
        let rel = guard::validate_tree_path(".github/CLAUDE.md").expect("rel");
        check_no_symlink_or_emitted_leaf(&root, &rel).expect("leaf link passes");
        assert!(emitted_leaf_symlink_only(&root, &rel));

        let root = scratch("ancestor_link").expect("scratch");
        let backing = root.join("backing-dir");
        std::fs::create_dir_all(&backing).expect("backing");
        std::os::unix::fs::symlink(&backing, root.join(".github")).expect("ancestor link");
        let rel = guard::validate_tree_path(".github/CLAUDE.md").expect("rel");
        let err = check_no_symlink_or_emitted_leaf(&root, &rel).expect_err("ancestor fails");
        assert!(err.to_string().contains("symlink_prefix"), "{err}");
        assert!(!emitted_leaf_symlink_only(&root, &rel));
    }
}
