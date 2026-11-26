// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, commit, finish, git};

fn plant_hook(dir: &Path, name: &str, marker: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let hook = dir.join(name);
    std::fs::write(
        &hook,
        format!("#!/bin/sh\necho ran > '{}'\n", marker.display()),
    )?;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn warm_mirror_then_advance(rig: &Rig) -> Result<String> {
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    rig.advance("next")
}

#[test]
fn planted_mirror_hook_does_not_run() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let marker = rig.root.join("hook-ran");
    plant_hook(
        &rig.mirror().join("hooks"),
        "reference-transaction",
        &marker,
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(!marker.exists(), "the mirror's hook ran");
    Ok(())
}

#[test]
fn planted_mirror_config_is_replaced_by_the_template() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let marker = rig.root.join("hook-ran");
    let evil_hooks = rig.root.join("evil-hooks");
    plant_hook(&evil_hooks, "reference-transaction", &marker)?;
    let config = rig.mirror().join("config");
    let mut planted = std::fs::OpenOptions::new().append(true).open(&config)?;
    write!(
        planted,
        "[core]\n\thooksPath = {}\n[url \"file:///nowhere/\"]\n\tinsteadOf = file://\n",
        evil_hooks.display()
    )?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    assert!(!marker.exists(), "a hook from the planted config ran");
    let after = std::fs::read_to_string(&config)?;
    assert!(
        !after.contains("insteadOf") && !after.contains("hooksPath"),
        "{after}"
    );
    Ok(())
}

#[test]
fn host_git_config_does_not_apply() -> Result<()> {
    let rig = Rig::new()?;
    let global = rig.root.join("global-gitconfig");
    std::fs::write(
        &global,
        "[url \"file:///nowhere/\"]\n\tinsteadOf = file://\n",
    )?;
    let job = rig.job("checkout_depth1", &rig.head, &rig.server_url())?;
    let scratch = Scratch::new()?;
    let env = [("GIT_CONFIG_GLOBAL", global.to_str().context("utf-8")?)];
    let ran = finish(rig.start_env(&job, &scratch, &env)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, rig.head);
    Ok(())
}

#[test]
fn stale_ref_locks_are_removed_before_the_fetch() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    let heads = rig.mirror().join("refs/heads");
    let locks = [heads.join("main.lock"), heads.join("nested/other.lock")];
    std::fs::create_dir_all(heads.join("nested"))?;
    for lock in &locks {
        std::fs::write(lock, "")?;
    }
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?, next);
    assert!(locks.iter().all(|lock| !lock.exists()));
    Ok(())
}

#[test]
fn failed_fetch_that_stored_the_commit_still_succeeds() -> Result<()> {
    let rig = Rig::new()?;
    let next = warm_mirror_then_advance(&rig)?;
    std::fs::write(rig.mirror().join("refs/heads/main"), "not a commit id\n")?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth1", &next, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(git(&scratch.workspace(), &["rev-parse", "HEAD"])?, next);
    Ok(())
}

fn checkout(rig: &Rig, job: &Path) -> Result<(Option<i32>, String)> {
    let scratch = Scratch::new()?;
    let ran = finish(rig.start(job, &scratch)?)?;
    Ok((
        ran.code,
        git(&scratch.workspace(), &["rev-parse", "HEAD"]).unwrap_or_default(),
    ))
}

#[test]
fn mirror_is_keyed_by_host_owner_and_repository() -> Result<()> {
    let rig = Rig::new()?;
    let other = rig.root.join("up/someone/velnor-new.git");
    let work = rig.root.join("other-work");
    std::fs::create_dir_all(&other)?;
    std::fs::create_dir_all(&work)?;
    git(&other, &["init", "-q", "--bare"])?;
    git(&work, &["init", "-q", "-b", "main"])?;
    let theirs = commit(&work, "theirs")?;
    git(&work, &["push", "-q", &other.to_string_lossy(), "main"])?;
    let closed = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let elsewhere = format!("http://127.0.0.1:{closed}");
    let someone = [("tailrocks/velnor-new", "someone/velnor-new")];
    let url = rig.server_url();
    let ours = (
        rig.job("checkout_depth1", &rig.head, &url)?,
        rig.head.clone(),
    );
    let cases = [
        (&ours.0, (Some(0), rig.head.clone())),
        (
            &rig.job("checkout_depth1", &rig.head, &elsewhere)?,
            (Some(1), String::new()),
        ),
        (
            &rig.job_edit("checkout_depth1", &theirs, &url, &someone)?,
            (Some(0), theirs.clone()),
        ),
        (
            &rig.job_edit("checkout_depth1", &rig.head, &url, &someone)?,
            (Some(1), String::new()),
        ),
        (
            &rig.job("checkout_depth1", &theirs, &url)?,
            (Some(1), String::new()),
        ),
        (&ours.0, (Some(0), ours.1.clone())),
    ];
    for (job, expected) in cases {
        assert_eq!(checkout(&rig, job)?, expected);
    }
    Ok(())
}

#[test]
fn persist_credentials_other_than_true_is_accepted_as_false() -> Result<()> {
    let rig = Rig::new()?;
    for (spelling, accepted) in [
        ("False", true),
        ("FALSE", true),
        ("fAlse", true),
        ("no", true),
        ("", true),
        ("true", false),
        ("TRUE", false),
        ("True", false),
    ] {
        let job = rig.job_edit(
            "checkout_depth1",
            &rig.head,
            &rig.server_url(),
            &[("\"lit\": \"false\"", &format!("\"lit\": \"{spelling}\""))],
        )?;
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        let expected = if accepted { Some(0) } else { Some(1) };
        assert_eq!(
            ran.code, expected,
            "{spelling}: {}\n{}",
            ran.stdout, ran.stderr
        );
    }
    Ok(())
}

#[test]
fn checkout_fetch_uses_trusted_facts() -> Result<()> {
    let rig = Rig::new()?;
    let closed = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let facts = rig.root.join("trusted.json");
    std::fs::write(
        &facts,
        format!(
            r#"{{"repository_id": 1, "repository": "tailrocks/velnor-new",
"server_url": "{}", "default_branch": "main", "sha": "{}", "run_id": 1,
"run_attempt": 1, "event": {{"kind": "pull_request", "number": 7,
"base_branch": "main", "head": "same_repository"}}}}"#,
            rig.server_url(),
            rig.merge
        ),
    )?;
    let lies = [
        ("refs/pull/7/merge", "refs/pull/9/merge"),
        ("tailrocks/velnor-new", "someone/else"),
    ];
    let job = rig.job_edit(
        "checkout_pull_request",
        &rig.merge,
        &format!("http://127.0.0.1:{closed}"),
        &lies,
    )?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start_trusted(&job, &facts, &scratch)?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(
        git(&scratch.workspace(), &["rev-parse", "HEAD"])?,
        rig.merge
    );
    let refs = git(
        &rig.mirror_of(1),
        &["for-each-ref", "--format=%(refname)", "refs/pull"],
    )?;
    assert_eq!(refs, "refs/pull/7/merge");
    Ok(())
}

#[test]
fn a_hangup_interrupt_or_terminate_while_another_process_holds_the_mirror_lock_exits_cancelled()
-> Result<()> {
    let rig = Rig::new()?;
    let mut lock_path = rig.mirror().into_os_string();
    lock_path.push(".lock");
    let lock_path = std::path::PathBuf::from(lock_path);
    std::fs::create_dir_all(lock_path.parent().context("lock parent")?)?;
    let holder = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    holder.lock()?;
    for signal in ["-TERM", "-INT", "-HUP"] {
        let scratch = Scratch::new()?;
        let mut runner = rig.start(
            &rig.job("checkout_depth1", &rig.head, &rig.server_url())?,
            &scratch,
        )?;
        std::thread::sleep(Duration::from_secs(1));
        assert!(runner.try_wait()?.is_none(), "the runner did not wait");
        let signalled = Command::new("kill")
            .args([signal, &runner.id().to_string()])
            .status()
            .context("run kill")?;
        assert!(signalled.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        while runner.try_wait()?.is_none() {
            if Instant::now() > deadline {
                let _ = runner.kill();
                let _ = runner.wait();
                bail!("the runner did not exit on {signal} while the mirror lock was held");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let ran = finish(runner)?;
        assert_eq!(
            ran.code,
            Some(1),
            "{signal}: {}\n{}",
            ran.stdout,
            ran.stderr
        );
        assert!(ran.stdout.contains("cancelled"), "{signal}: {}", ran.stdout);
    }
    drop(holder);
    Ok(())
}

fn trusted_facts(
    rig: &Rig,
    repository_id: u64,
    sha: &str,
    event: &str,
) -> Result<std::path::PathBuf> {
    let path = rig.root.join(format!("trusted-{repository_id}-{sha}.json"));
    std::fs::write(
        &path,
        format!(
            r#"{{"repository_id": {repository_id}, "repository": "tailrocks/velnor-new",
"server_url": "{}", "default_branch": "main", "sha": "{sha}", "run_id": 1,
"run_attempt": 1, "event": {event}}}"#,
            rig.server_url()
        ),
    )?;
    Ok(path)
}

fn checkout_trusted(rig: &Rig, facts: &Path) -> Result<(Option<i32>, String)> {
    let job = rig.job("checkout_depth1", "a", &rig.server_url())?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start_trusted(&job, facts, &scratch)?)?;
    Ok((
        ran.code,
        git(&scratch.workspace(), &["rev-parse", "HEAD"]).unwrap_or_default(),
    ))
}

#[test]
fn a_recreated_repository_does_not_see_the_old_mirror() -> Result<()> {
    let rig = Rig::new()?;
    let branch = r#"{"kind": "branch", "branch": "main"}"#;
    let old = trusted_facts(&rig, 1, &rig.head, branch)?;
    let new = trusted_facts(&rig, 2, &rig.head, branch)?;
    assert_eq!(checkout_trusted(&rig, &old)?, (Some(0), rig.head.clone()));
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    std::fs::remove_dir_all(&upstream)?;
    std::fs::create_dir_all(&upstream)?;
    git(&upstream, &["init", "-q", "--bare"])?;
    let work = rig.root.join("recreated");
    std::fs::create_dir_all(&work)?;
    git(&work, &["init", "-q", "-b", "main"])?;
    commit(&work, "recreated")?;
    git(&work, &["push", "-q", &upstream.to_string_lossy(), "main"])?;
    assert_eq!(checkout_trusted(&rig, &old)?, (Some(0), rig.head.clone()));
    assert_eq!(checkout_trusted(&rig, &new)?, (Some(1), String::new()));
    assert!(rig.mirror_of(1).is_dir() && rig.mirror_of(2).is_dir());
    Ok(())
}

#[test]
fn a_tag_job_fetches_its_tag_when_no_branch_reaches_the_commit() -> Result<()> {
    let rig = Rig::new()?;
    let work = rig.root.join("work");
    let upstream = rig
        .root
        .join("up/tailrocks/velnor-new.git")
        .to_string_lossy()
        .into_owned();
    git(&work, &["checkout", "-q", "--detach"])?;
    let tagged = commit(&work, "tagged")?;
    git(&work, &["tag", "v1"])?;
    git(&work, &["tag", "v2", &rig.head])?;
    git(
        &work,
        &["push", "-q", &upstream, "refs/tags/v1", "refs/tags/v2"],
    )?;
    git(&work, &["checkout", "-q", "main"])?;
    let facts = trusted_facts(&rig, 5, &tagged, r#"{"kind": "tag", "name": "v1"}"#)?;
    assert_eq!(checkout_trusted(&rig, &facts)?, (Some(0), tagged));
    let tags = git(
        &rig.mirror_of(5),
        &["for-each-ref", "--format=%(refname)", "refs/tags"],
    )?;
    assert_eq!(tags, "refs/tags/v1");
    Ok(())
}
