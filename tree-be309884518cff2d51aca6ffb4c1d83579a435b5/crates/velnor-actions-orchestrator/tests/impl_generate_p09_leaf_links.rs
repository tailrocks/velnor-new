//! P09 regression cases: legacy symlinks at emitted leaves are replaced
//! by the fresh generated file instead of refusing generation.
//!
//! Owned by the leaf-link builder; wired into `velnor_orchestrator` by
//! the orchestrator-test owner with one `mod` line.

use std::fs;

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use super::impl_common::{TestResult, config_with_branch, make_repo};

#[cfg(unix)]
#[test]
fn in_place_emitted_leaf_symlink_replaced_by_fresh_file() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let github = root.join(".github");
    fs::create_dir_all(github.join("workflows"))?;
    // Legacy symlink-era entry at an emitted leaf; generation replaces it
    // with the fresh pointer file instead of refusing.
    std::os::unix::fs::symlink("AGENTS.md", github.join("CLAUDE.md"))?;
    let prep = prepare(root)?;
    let opts = GenerateOptions { output_dir: None };
    generate(&prep, &opts)?;
    let claude = github.join("CLAUDE.md");
    assert!(
        !fs::symlink_metadata(&claude)?.is_symlink(),
        "leaf symlink replaced by a regular file"
    );
    assert_eq!(fs::read(&claude)?, b"@AGENTS.md\n");
    Ok(())
}
