// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path, time::Duration};

use super::validate_history;
use crate::git_fixture;
use tempfile::TempDir;
use velnor_actions_mise::CheckDeadline;

struct GitFixture {
    directory: TempDir,
    protected_base: String,
    base: String,
    direct_head: String,
    merge_head: String,
    two_commits_head: String,
    unrelated_head: String,
    landed: String,
}

impl GitFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        git(directory.path(), &["init", "--quiet"])?;
        let empty_tree = git(directory.path(), &["mktree"])?;
        let protected_base = commit(directory.path(), &empty_tree, &[], "protected base")?;
        let base = commit(
            directory.path(),
            &empty_tree,
            &[&protected_base],
            "pull base",
        )?;
        let direct_head = commit(directory.path(), &empty_tree, &[&base], "direct head")?;
        let side = commit(directory.path(), &empty_tree, &[&base], "merge side")?;
        let merge_head = commit(directory.path(), &empty_tree, &[&base, &side], "merge head")?;
        let first = commit(directory.path(), &empty_tree, &[&base], "first head commit")?;
        let two_commits_head = commit(
            directory.path(),
            &empty_tree,
            &[&first],
            "second head commit",
        )?;
        let unrelated_root = commit(directory.path(), &empty_tree, &[], "unrelated root")?;
        let unrelated_head = commit(
            directory.path(),
            &empty_tree,
            &[&unrelated_root],
            "unrelated head",
        )?;
        let landed = commit(directory.path(), &empty_tree, &[&base], "landed")?;
        git(
            directory.path(),
            &["update-ref", "refs/remotes/origin/main", &landed],
        )?;
        Ok(Self {
            directory,
            protected_base,
            base,
            direct_head,
            merge_head,
            two_commits_head,
            unrelated_head,
            landed,
        })
    }

    fn validate(&self, head: &str) -> Result<(), crate::OrchestratorError> {
        validate_history(
            self.directory.path(),
            &self.protected_base,
            &self.base,
            head,
            &self.landed,
            CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
        )
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let mut command = if root.join(".git").exists() {
        git_fixture::command(root)?
    } else {
        git_fixture::sterile_command()
    };
    let output = command
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!("git {args:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn commit(
    root: &Path,
    tree: &str,
    parents: &[&str],
    message: &str,
) -> Result<String, Box<dyn Error>> {
    let mut args = vec!["commit-tree", tree];
    for parent in parents {
        args.extend(["-p", parent]);
    }
    args.extend(["-m", message]);
    git(root, &args)
}

#[test]
fn cutover_checkout_history_accepts_direct_single_parent_head() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;

    assert!(fixture.validate(&fixture.direct_head).is_ok());
    Ok(())
}

#[test]
fn cutover_checkout_history_rejects_two_parent_merge_head() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;

    assert!(fixture.validate(&fixture.merge_head).is_err());
    Ok(())
}

#[test]
fn cutover_checkout_history_rejects_head_two_commits_above_base() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;

    assert!(fixture.validate(&fixture.two_commits_head).is_err());
    Ok(())
}

#[test]
fn cutover_checkout_history_rejects_unrelated_single_parent_head() -> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;

    assert!(fixture.validate(&fixture.unrelated_head).is_err());
    Ok(())
}
