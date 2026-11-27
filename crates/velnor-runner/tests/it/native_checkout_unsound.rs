// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};
use crate::native_checkout_env::wrapper_env;
use crate::native_checkout_quarantine::quarantined;
use crate::native_checkout_repair::{object_path, overwrite};

const PLAIN_FETCH_BRINGS_NOTHING: &str =
    "case \" $* \" in *\" --refetch \"*) ;; *\" fetch \"*) exit 0;; esac\n";

fn warm_behind_a_second_commit(rig: &Rig) -> Result<()> {
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let next = rig.advance("third")?;
    let ahead = rig.run("checkout_depth1", &next, &Scratch::new()?)?;
    assert_eq!(ahead.code, Some(0), "{}\n{}", ahead.stdout, ahead.stderr);
    Ok(())
}

fn checks_out(rig: &Rig, fixture: &str, sha: &str, commits: &str) -> Result<()> {
    let scratch = Scratch::new()?;
    let ran = rig.run(fixture, sha, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let workspace = scratch.workspace();
    assert_eq!(git(&workspace, &["rev-parse", "HEAD"])?, sha);
    assert_eq!(git(&workspace, &["rev-list", "--count", "HEAD"])?, commits);
    assert_eq!(std::fs::read_to_string(workspace.join("second"))?, "second");
    Ok(())
}

#[test]
fn a_missing_ancestor_commit_behind_a_live_ref_is_refetched_once() -> Result<()> {
    let rig = Rig::new()?;
    warm_behind_a_second_commit(&rig)?;
    std::fs::remove_file(object_path(&rig.mirror(), &rig.head))?;
    for _ in 0..2 {
        checks_out(&rig, "checkout_depth1", &rig.head, "1")?;
    }
    assert_eq!(rig.refetches(), 1);
    git(&rig.mirror(), &["cat-file", "-e", &rig.head])?;
    Ok(())
}

fn quarantine_then_fail_the_first_refetch(rig: &Rig, plain_fetch: &str) -> Result<()> {
    warm_behind_a_second_commit(rig)?;
    overwrite(&object_path(&rig.mirror(), &rig.head), b"")?;
    let once = rig.root.join("refetch-failed-once");
    std::fs::write(&once, "")?;
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in\n\
             *\" --refetch \"*) [ -f '{once}' ] && {{ echo transient >&2; exit 128; }};;\n\
             *\" fetch \"*) {plain_fetch};;\n\
             esac\n",
            once = once.display()
        ),
    )?;
    let failed = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(failed.code, Some(1), "{}\n{}", failed.stdout, failed.stderr);
    assert_eq!(rig.refetches(), 0);
    std::fs::remove_file(&once)?;
    Ok(())
}

#[test]
fn a_commit_quarantined_before_a_failed_refetch_is_refetched_by_the_next_job() -> Result<()> {
    let rig = Rig::new()?;
    quarantine_then_fail_the_first_refetch(&rig, ":")?;
    for _ in 0..2 {
        checks_out(&rig, "checkout_depth1", &rig.head, "1")?;
    }
    assert_eq!(rig.refetches(), 1);
    assert_eq!(quarantined(&rig).len(), 1);
    Ok(())
}

#[test]
fn a_quarantined_commit_is_refetched_when_the_next_fetch_brings_nothing() -> Result<()> {
    let rig = Rig::new()?;
    quarantine_then_fail_the_first_refetch(&rig, "exit 0")?;
    checks_out(&rig, "checkout_depth1", &rig.head, "1")?;
    assert_eq!(rig.refetches(), 1);
    Ok(())
}

#[test]
fn an_empty_job_commit_behind_a_branch_is_refetched_when_the_fetch_brings_nothing() -> Result<()> {
    let rig = Rig::new()?;
    warm_behind_a_second_commit(&rig)?;
    overwrite(&object_path(&rig.mirror(), &rig.head), b"")?;
    wrapper_env(&rig.root, PLAIN_FETCH_BRINGS_NOTHING)?;
    checks_out(&rig, "checkout_depth1", &rig.head, "1")?;
    assert_eq!(rig.refetches(), 1);
    assert_eq!(quarantined(&rig).len(), 1);
    Ok(())
}

fn warm_then_damage_the_parent(
    rig: &Rig,
    spec: &str,
    damage: fn(&std::path::Path) -> Result<()>,
) -> Result<(String, String)> {
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let next = rig.advance("third")?;
    let ahead = rig.run("checkout_depth1", &next, &Scratch::new()?)?;
    assert_eq!(ahead.code, Some(0), "{}\n{}", ahead.stdout, ahead.stderr);
    let id = git(&rig.mirror(), &["rev-parse", spec])?;
    damage(&object_path(&rig.mirror(), &id))?;
    Ok((next, id))
}

fn empty(object: &std::path::Path) -> Result<()> {
    overwrite(object, b"")
}

fn removed(object: &std::path::Path) -> Result<()> {
    Ok(std::fs::remove_file(object)?)
}

fn repairs_at_depth_two(rig: &Rig, next: &str) -> Result<()> {
    for _ in 0..2 {
        checks_out(rig, "checkout_depth2", next, "2")?;
    }
    assert_eq!(rig.refetches(), 1);
    Ok(())
}

#[test]
fn a_depth_two_checkout_repairs_an_empty_parent_commit() -> Result<()> {
    let rig = Rig::new()?;
    let (next, parent) = warm_then_damage_the_parent(&rig, &rig.head, empty)?;
    repairs_at_depth_two(&rig, &next)?;
    let moved = quarantined(&rig);
    assert!(
        moved.len() == 1 && moved[0].starts_with(&parent),
        "{moved:?}"
    );
    Ok(())
}

#[test]
fn a_depth_two_checkout_repairs_a_missing_parent_commit() -> Result<()> {
    let rig = Rig::new()?;
    let (next, _) = warm_then_damage_the_parent(&rig, &rig.head, removed)?;
    repairs_at_depth_two(&rig, &next)?;
    assert_eq!(quarantined(&rig), Vec::<String>::new());
    Ok(())
}

#[test]
fn a_depth_two_checkout_repairs_an_empty_tree_of_the_parent_commit() -> Result<()> {
    let rig = Rig::new()?;
    let spec = format!("{}^{{tree}}", rig.head);
    let (next, tree) = warm_then_damage_the_parent(&rig, &spec, empty)?;
    repairs_at_depth_two(&rig, &next)?;
    let moved = quarantined(&rig);
    assert!(moved.len() == 1 && moved[0].starts_with(&tree), "{moved:?}");
    Ok(())
}
