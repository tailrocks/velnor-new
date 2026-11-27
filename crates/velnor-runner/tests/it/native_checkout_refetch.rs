// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish, git};
use crate::native_checkout_env::wrapper_env;

fn said(ran: &Ran, text: &str) -> bool {
    ran.stdout.contains(text) || ran.stderr.contains(text)
}

fn lose_a_blob(rig: &Rig) -> Result<()> {
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let id = git(
        &rig.mirror(),
        &["rev-parse", &format!("{}:second", rig.head)],
    )?;
    std::fs::remove_file(rig.mirror().join("objects").join(&id[..2]).join(&id[2..]))?;
    Ok(())
}

fn refetch_fails_with(rig: &Rig, text: &str, code: u8) -> Result<Ran> {
    lose_a_blob(rig)?;
    wrapper_env(
        &rig.root,
        &format!("case \" $* \" in *\" --refetch \"*) echo \"{text}\" >&2; exit {code};; esac\n"),
    )?;
    rig.run("checkout_depth1", &rig.head, &Scratch::new()?)
}

#[test]
fn a_commit_the_server_does_not_have_fails_without_a_refetch_or_a_version_hint() -> Result<()> {
    let rig = Rig::new()?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &"a".repeat(40), &scratch)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(said(&ran, "not our ref"), "{}\n{}", ran.stdout, ran.stderr);
    assert!(!said(&ran, "2.36"), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(rig.fetches(), 2);
    Ok(())
}

#[test]
fn the_git_version_is_named_when_git_does_not_know_refetch() -> Result<()> {
    let rig = Rig::new()?;
    let ran = refetch_fails_with(&rig, "error: unknown option \\`refetch'", 129)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        said(&ran, "needs git 2.36 or newer"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    Ok(())
}

#[test]
fn another_refetch_failure_does_not_blame_the_git_version() -> Result<()> {
    let rig = Rig::new()?;
    let ran = refetch_fails_with(&rig, "fatal: the server went away", 128)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        said(&ran, "the server went away"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!said(&ran, "2.36"), "{}\n{}", ran.stdout, ran.stderr);
    Ok(())
}

#[test]
fn a_broken_ref_outside_the_job_refspecs_is_removed_before_the_fetch() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let mirror = rig.mirror();
    let broken = mirror.join("refs/pull/9/merge");
    let sound = mirror.join("refs/keep/head");
    std::fs::create_dir_all(broken.parent().expect("ref directory"))?;
    std::fs::create_dir_all(sound.parent().expect("ref directory"))?;
    std::fs::write(&broken, format!("{}\n", "1".repeat(40)))?;
    std::fs::write(&sound, format!("{}\n", rig.head))?;
    let next = rig.advance("third")?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(
        ran.stdout.contains("removed refs/pull/9/merge"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    assert!(!broken.exists());
    assert_eq!(git(&mirror, &["rev-parse", "refs/keep/head"])?, rig.head);
    assert_eq!(rig.fetches(), 2);
    Ok(())
}

#[test]
fn a_commit_only_an_annotated_tag_reaches_is_repaired_by_a_refetch() -> Result<()> {
    let rig = Rig::new()?;
    let work = rig.root.join("work");
    let first = git(&work, &["rev-parse", &format!("{}~1", rig.head)])?;
    git(&work, &["tag", "-a", "-m", "v1", "v1", &first])?;
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    git(&work, &["push", "-q", &upstream.to_string_lossy(), "v1"])?;
    let edits = [("\"s\": \"refs/heads/main\"", "\"s\": \"refs/tags/v1\"")];
    let job = rig.job_edit("checkout_depth1", &first, &rig.server_url(), &edits)?;
    let warm = finish(rig.start(&job, &Scratch::new()?)?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let mirror = rig.mirror();
    let id = git(&mirror, &["rev-parse", &format!("{first}:first")])?;
    std::fs::remove_file(mirror.join("objects").join(&id[..2]).join(&id[2..]))?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(&job, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, first);
    git(&mirror, &["cat-file", "-e", &id])?;
    Ok(())
}

fn packs(mirror: &std::path::Path) -> Result<usize> {
    Ok(std::fs::read_dir(mirror.join("objects/pack"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "pack"))
        .count())
}

fn warm_and_break_main(rig: &Rig) -> Result<()> {
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    std::fs::write(rig.root.join("packed"), "")?;
    std::fs::write(
        rig.mirror().join("refs/heads/main"),
        format!("{}\n", "1".repeat(40)),
    )?;
    Ok(())
}

#[test]
fn a_broken_ref_the_job_fetches_is_removed_and_main_advances_without_new_packs() -> Result<()> {
    let rig = Rig::new()?;
    warm_and_break_main(&rig)?;
    let mirror = rig.mirror();
    let mut counts = Vec::new();
    for file in ["third", "fourth"] {
        let next = rig.advance(file)?;
        let scratch = Scratch::new()?;
        let ran = rig.run("checkout_depth1", &next, &scratch)?;
        assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
        assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
        assert_eq!(git(&mirror, &["rev-parse", "refs/heads/main"])?, next);
        assert_eq!(rig.refetches(), 0);
        counts.push((said(&ran, "removed refs/heads/main"), packs(&mirror)?));
    }
    assert!(counts[0].0 && !counts[1].0, "{counts:?}");
    assert_eq!(counts[0].1 + 1, counts[1].1, "{counts:?}");
    Ok(())
}

#[test]
fn a_dropped_broken_ref_triggers_a_refetch_when_the_fetch_brought_nothing() -> Result<()> {
    let rig = Rig::new()?;
    warm_and_break_main(&rig)?;
    let next = rig.advance("third")?;
    wrapper_env(
        &rig.root,
        "case \" $* \" in *\" --refetch \"*) ;; *\" fetch \"*) exit 0;; esac\n",
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(said(&ran, "removed refs/heads/main"));
    Ok(())
}
