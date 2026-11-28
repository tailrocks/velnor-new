// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, commit, finish, git};
use crate::native_checkout_quarantine::quarantined;
use crate::native_checkout_repair::{object_path, overwrite};
use crate::native_checkout_unsound::{
    checks_out, quarantine_then_fail_the_first_refetch, warm_behind_a_second_commit,
};

fn checks_out_deep(rig: &Rig, sha: &str, depth: &str, commits: &str) -> Result<Scratch> {
    let job = rig.job_edit(
        "checkout_depth2",
        sha,
        &rig.server_url(),
        &[("\"lit\": \"2\"", &format!("\"lit\": \"{depth}\""))],
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, sha);
    assert_eq!(git(&workspace, &["rev-list", "--count", "HEAD"])?, commits);
    Ok(scratch)
}

fn repairs_a_full_checkout_once(rig: &Rig, next: &str) -> Result<()> {
    for _ in 0..2 {
        let scratch = checks_out_deep(rig, next, "0", "3")?;
        git(&scratch.workspace(), &["rev-list", "--objects", "HEAD"])?;
    }
    assert_eq!(rig.refetches(), 1);
    git(&rig.mirror(), &["rev-list", "--objects", "--quiet", next])?;
    Ok(())
}

fn warm_full_history(rig: &Rig) -> Result<String> {
    warm_behind_a_second_commit(rig)?;
    git(&rig.mirror(), &["rev-parse", "refs/heads/main"])
}

#[test]
fn a_full_checkout_repairs_a_missing_parent_commit() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_full_history(&rig)?;
    std::fs::remove_file(object_path(&rig.mirror(), &rig.head))?;
    repairs_a_full_checkout_once(&rig, &next)
}

#[test]
fn a_full_checkout_repairs_an_empty_parent_commit() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_full_history(&rig)?;
    overwrite(&object_path(&rig.mirror(), &rig.head), b"")?;
    repairs_a_full_checkout_once(&rig, &next)?;
    let moved = quarantined(&rig);
    assert!(
        moved.len() == 1 && moved[0].starts_with(&rig.head),
        "{moved:?}"
    );
    Ok(())
}

#[test]
fn a_full_checkout_repairs_a_commit_lost_after_a_failed_refetch() -> Result<()> {
    let rig = Rig::new()?;
    quarantine_then_fail_the_first_refetch(&rig, ":")?;
    let next = git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?;
    assert!(git(&rig.mirror(), &["cat-file", "-e", &rig.head]).is_err());
    repairs_a_full_checkout_once(&rig, &next)
}

fn side_branch_merge(rig: &Rig) -> Result<(String, String)> {
    let work = rig.root.join("work");
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    git(&work, &["checkout", "-q", "-b", "side", &rig.head])?;
    let side = commit(&work, "side")?;
    git(&work, &["checkout", "-q", "main"])?;
    commit(&work, "main2")?;
    git(&work, &["merge", "-q", "--no-ff", "-m", "m", "side"])?;
    let merge = git(&work, &["rev-parse", "HEAD"])?;
    git(&work, &["push", "-q", &upstream.to_string_lossy(), "main"])?;
    let warm = rig.run("checkout_depth1", &merge, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    Ok((merge, side))
}

fn repairs_the_second_parent_of_a_merge(
    spec: impl FnOnce(&str) -> String,
    damage: fn(&Path) -> Result<()>,
) -> Result<()> {
    let rig = Rig::new()?;
    let (merge, side) = side_branch_merge(&rig)?;
    let id = git(&rig.mirror(), &["rev-parse", &spec(&side)])?;
    damage(&object_path(&rig.mirror(), &id))?;
    for _ in 0..2 {
        checks_out(&rig, "checkout_depth2", &merge, "3")?;
    }
    assert_eq!(rig.refetches(), 1);
    git(&rig.mirror(), &["rev-list", "--objects", "--quiet", &merge])?;
    Ok(())
}

fn empty(path: &Path) -> Result<()> {
    overwrite(path, b"")
}

fn removed(path: &Path) -> Result<()> {
    Ok(std::fs::remove_file(path)?)
}

fn tree_of(commit: &str) -> String {
    format!("{commit}^{{tree}}")
}

#[test]
fn a_depth_two_checkout_repairs_an_empty_tree_of_the_second_parent() -> Result<()> {
    repairs_the_second_parent_of_a_merge(tree_of, empty)
}

#[test]
fn a_depth_two_checkout_repairs_a_missing_tree_of_the_second_parent() -> Result<()> {
    repairs_the_second_parent_of_a_merge(tree_of, removed)
}

#[test]
fn a_depth_three_checkout_repairs_an_empty_tree_of_the_grandparent() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.merge, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let root = git(&rig.mirror(), &["rev-list", "--max-parents=0", &rig.merge])?;
    let tree = git(&rig.mirror(), &["rev-parse", &tree_of(&root)])?;
    empty(&object_path(&rig.mirror(), &tree))?;
    checks_out_deep(&rig, &rig.merge, "3", "4")?;
    assert_eq!(rig.refetches(), 1);
    Ok(())
}
