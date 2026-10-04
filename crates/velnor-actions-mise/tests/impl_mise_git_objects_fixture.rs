//! Owned object and tree fixtures for Discovery Git integration tests.

#[cfg(test)]
#[path = "impl_mise_git_index_fixture.rs"]
mod base;

pub(crate) use base::{git_dir, git_output, git_owned, install_hook, repo};

use std::path::Path;

/// Resolve one complete object ID through the sterile fixture Git authority.
pub(crate) fn object_id(root: &Path, spec: &str) -> Result<String, String> {
    Ok(git_output(root, &["rev-parse", spec])?.trim().to_owned())
}

/// Create an unrelated root commit in the same object database.
pub(crate) fn independent_commit(root: &Path) -> Result<String, String> {
    let tree = object_id(root, "HEAD^{tree}")?;
    Ok(
        git_output(root, &["commit-tree", &tree, "-m", "unrelated-history"])?
            .trim()
            .to_owned(),
    )
}

/// Add a real mode-160000 tree entry and return the new commit tree ID.
pub(crate) fn add_gitlink(root: &Path, child: &Path, path: &str) -> Result<String, String> {
    let child_head = object_id(child, "HEAD")?;
    let cacheinfo = format!("160000,{child_head},{path}");
    git_owned(
        root,
        vec![
            "update-index".to_owned(),
            "--add".to_owned(),
            "--cacheinfo".to_owned(),
            cacheinfo,
        ],
    )?;
    git_owned(
        root,
        vec![
            "commit".to_owned(),
            "-m".to_owned(),
            "add gitlink".to_owned(),
        ],
    )?;
    object_id(root, "HEAD^{tree}")
}
