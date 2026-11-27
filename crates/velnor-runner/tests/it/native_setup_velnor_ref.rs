// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, finish, git};
use crate::native_setup_velnor_fetch::{
    Cold, RAN, default_branch_commit, foreign_commit, upstream,
};

const MAIN: &str = r#"{"kind": "branch", "branch": "main"}"#;
const QUEUE: &str = "refs/heads/gh-readonly-queue/main/pr-1";

pub(crate) fn facts(rig: &Rig, event_name: &str, event: &str, sha: &str) -> Result<PathBuf> {
    let path = rig.root.join(format!("facts-{event_name}-{sha}.json"));
    std::fs::write(
        &path,
        format!(
            r#"{{"repository_id": 1, "repository": "tailrocks/velnor-new",
"server_url": "{}", "default_branch": "main", "sha": "{sha}", "run_id": 1,
"run_attempt": 1, "event_name": "{event_name}", "event": {event}}}"#,
            rig.server_url()
        ),
    )?;
    Ok(path)
}

fn checkout(rig: &Rig, facts: &std::path::Path, sha: &str, edits: &[(&str, &str)]) -> Result<Ran> {
    let job = rig.job_edit("checkout_depth1", sha, &rig.server_url(), edits)?;
    let scratch = Scratch::new()?;
    finish(rig.start_trusted(&job, facts, &scratch)?)
}

fn assert_unreachable(ran: &Ran, pin: &str, from: &str) {
    let message = format!("setup-velnor: the pin {pin} is not reachable from {from}");
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(ran.has_line("step 1 failed"), "{}", ran.stdout);
    assert!(
        ran.lines().iter().any(|line| line.contains(&message)),
        "{message}\n{}",
        ran.stdout
    );
    assert!(!ran.has_line(RAN), "{}", ran.stdout);
}

fn assert_ran(ran: &Ran) {
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert!(ran.has_line(RAN), "{}", ran.stdout);
}

#[test]
fn a_pin_left_in_the_mirror_by_a_tag_deleted_upstream_is_not_trusted() -> Result<()> {
    let rig = Rig::new()?;
    let tagged = foreign_commit(&rig, "refs/tags/v1")?;
    let cold = Cold::new(&rig)?;
    let tag = facts(&rig, "push", r#"{"kind": "tag", "name": "v1"}"#, &tagged)?;
    let fetched = checkout(&rig, &tag, &tagged, &[])?;
    assert_eq!(
        fetched.code,
        Some(0),
        "{}\n{}",
        fetched.stdout,
        fetched.stderr
    );
    assert_ran(&cold.run_as(&tagged, &tag)?);
    git(&upstream(&rig), &["update-ref", "-d", "refs/tags/v1"])?;
    assert!(cold.mirror_refs()?.contains("refs/tags/v1"));
    let main = facts(&rig, "push", MAIN, &rig.head)?;
    let ran = cold.run_as(&tagged, &main)?;
    assert_unreachable(&ran, &tagged, "refs/heads/main");
    assert!(cold.mirror_has(&tagged));
    Ok(())
}

#[test]
fn a_dispatch_on_a_tag_trusts_the_default_branch_and_not_the_tag() -> Result<()> {
    let rig = Rig::new()?;
    let tagged = foreign_commit(&rig, "refs/tags/v1")?;
    let cold = Cold::new(&rig)?;
    let event = r#"{"kind": "tag", "name": "v1"}"#;
    let dispatch = facts(&rig, "workflow_dispatch", event, &tagged)?;
    assert_unreachable(
        &cold.run_as(&tagged, &dispatch)?,
        &tagged,
        "refs/heads/main",
    );
    let push = facts(&rig, "push", event, &tagged)?;
    assert_ran(&cold.run_as(&tagged, &push)?);
    Ok(())
}

#[test]
fn no_trusted_ref_outlives_a_job_however_many_distinct_tags_the_jobs_run_on() -> Result<()> {
    let rig = Rig::new()?;
    let cold = Cold::new(&rig)?;
    for name in ["v1", "v2", "v3"] {
        let tagged = foreign_commit(&rig, &format!("refs/tags/{name}"))?;
        let event = format!(r#"{{"kind": "tag", "name": "{name}"}}"#);
        let tag = facts(&rig, "push", &event, &tagged)?;
        assert_ran(&cold.run_as(&tagged, &tag)?);
        assert_eq!(cold.trusted_refs()?, "", "{name}");
    }
    Ok(())
}

#[test]
fn a_merge_queue_branch_never_vouches_for_a_pin_even_while_it_is_left_in_the_mirror() -> Result<()>
{
    let rig = Rig::new()?;
    let queued = foreign_commit(&rig, QUEUE)?;
    let cold = Cold::new(&rig)?;
    let queue = r#"{"kind": "merge_group", "queue_branch": "gh-readonly-queue/main/pr-1"}"#;
    let group = facts(&rig, "merge_group", queue, &queued)?;
    let fetched = checkout(&rig, &group, &queued, &[])?;
    assert_eq!(
        fetched.code,
        Some(0),
        "{}\n{}",
        fetched.stdout,
        fetched.stderr
    );
    assert_unreachable(&cold.run_as(&queued, &group)?, &queued, "refs/heads/main");
    git(&upstream(&rig), &["update-ref", "-d", QUEUE])?;
    assert!(cold.mirror_refs()?.contains(QUEUE));
    let main = facts(&rig, "push", MAIN, &rig.head)?;
    assert_unreachable(&cold.run_as(&queued, &main)?, &queued, "refs/heads/main");
    assert!(cold.mirror_has(&queued));
    Ok(())
}

#[test]
fn a_fork_commit_left_in_a_warm_mirror_is_not_a_trusted_pin_until_it_is_merged() -> Result<()> {
    let rig = Rig::new()?;
    let trusted = default_branch_commit(&rig)?;
    let forked = foreign_commit(&rig, "refs/pull/9/merge")?;
    let cold = Cold::new(&rig)?;
    let pull = r#"{"kind": "pull_request", "number": 9, "base_branch": "main", "head": "fork"}"#;
    let request = facts(&rig, "pull_request", pull, &forked)?;
    let lies = [("refs/pull/7/merge", "refs/pull/9/merge")];
    let fetched = checkout(&rig, &request, &forked, &lies)?;
    assert_eq!(
        fetched.code,
        Some(0),
        "{}\n{}",
        fetched.stdout,
        fetched.stderr
    );
    assert!(cold.mirror_refs()?.contains("refs/pull/9/merge"));
    let main = facts(&rig, "push", MAIN, &rig.head)?;
    assert_ran(&cold.run_as(&trusted, &main)?);
    let ran = cold.run_as(&forked, &main)?;
    assert_unreachable(&ran, &forked, "refs/heads/main");
    assert!(cold.mirror_has(&forked));
    assert_eq!(cold.mirror_refs_containing(&forked)?, "");
    let target = upstream(&rig).to_string_lossy().into_owned();
    git(
        &rig.root
            .join(format!("foreign-{}", "refs/pull/9/merge".replace('/', "-"))),
        &["push", "-q", "--force", &target, "HEAD:refs/heads/main"],
    )?;
    assert_ran(&cold.run_as(&forked, &main)?);
    Ok(())
}

#[test]
fn a_pin_that_left_the_trusted_ref_upstream_is_not_trusted_through_a_stale_copy() -> Result<()> {
    let rig = Rig::new()?;
    let pinned = default_branch_commit(&rig)?;
    let cold = Cold::new(&rig)?;
    let main = facts(&rig, "push", MAIN, &rig.head)?;
    assert_ran(&cold.run_as(&pinned, &main)?);
    git(
        &upstream(&rig),
        &["update-ref", "refs/heads/main", &rig.head],
    )?;
    assert!(cold.mirror_has(&pinned));
    let ran = cold.run_as(&pinned, &main)?;
    assert_unreachable(&ran, &pinned, "refs/heads/main");
    Ok(())
}
