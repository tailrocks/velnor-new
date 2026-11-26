// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::Result;

use crate::exec_support::{Ran, Scratch};
use crate::native_checkout::{Rig, commit, finish, git};

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
"run_attempt": 1, "event_name": "push", "event": {event}}}"#,
            rig.server_url()
        ),
    )?;
    Ok(path)
}

fn checkout_ran(rig: &Rig, facts: &Path) -> Result<(Ran, String)> {
    let job = rig.job("checkout_depth1", "a", &rig.server_url())?;
    let scratch = Scratch::new()?;
    let ran = finish(rig.start_trusted(&job, facts, &scratch)?)?;
    let head = git(&scratch.workspace(), &["rev-parse", "HEAD"]).unwrap_or_default();
    Ok((ran, head))
}

fn checkout_trusted(rig: &Rig, facts: &Path) -> Result<(Option<i32>, String)> {
    let (ran, head) = checkout_ran(rig, facts)?;
    Ok((ran.code, head))
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
    let (ran, head) = checkout_ran(&rig, &new)?;
    assert_eq!((ran.code, head.as_str()), (Some(1), ""));
    assert!(
        ran.stdout.contains(&format!("not our ref {}", rig.head)),
        "{}",
        ran.stdout
    );
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

#[test]
fn a_branch_job_fetches_its_commit_when_no_head_reaches_it_any_more() -> Result<()> {
    let rig = Rig::new()?;
    let work = rig.root.join("work");
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    let target = upstream.to_string_lossy().into_owned();
    git(
        &upstream,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    )?;
    git(&work, &["checkout", "-q", "--detach"])?;
    let forced = commit(&work, "forced-away")?;
    git(
        &work,
        &["push", "-q", &target, &format!("{forced}:refs/heads/queue")],
    )?;
    git(&work, &["push", "-q", &target, ":refs/heads/queue"])?;
    git(&work, &["checkout", "-q", "main"])?;
    for (id, event) in [
        (6, r#"{"kind": "branch", "branch": "main"}"#),
        (
            7,
            r#"{"kind": "merge_group", "queue_branch": "gh-readonly-queue/main/pr-1"}"#,
        ),
    ] {
        let facts = trusted_facts(&rig, id, &forced, event)?;
        assert_eq!(
            checkout_trusted(&rig, &facts)?,
            (Some(0), forced.clone()),
            "{event}"
        );
    }
    Ok(())
}

#[test]
fn the_mirror_store_is_private_to_its_owner() -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let rig = Rig::new()?;
    let ran = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    let mut directory = rig.mirror();
    while directory != rig.store() {
        if directory != rig.mirror() {
            let mode = std::fs::metadata(&directory)?.mode();
            assert_eq!(mode & 0o077, 0, "{} is {mode:o}", directory.display());
        }
        assert!(directory.pop());
    }
    Ok(())
}

#[test]
fn a_job_without_a_usable_repository_id_creates_no_mirror() -> Result<()> {
    let rig = Rig::new()?;
    let url = rig.server_url();
    for edit in [
        ("\"k\": \"repository_id\"", "\"k\": \"renamed_id\""),
        (
            "\"s\": \"1\"\n          }\n        },\n        {\n          \"k\": \"event_name\"",
            "\"s\": \"0\"\n          }\n        },\n        {\n          \"k\": \"event_name\"",
        ),
    ] {
        let job = rig.job_edit("checkout_depth1", &rig.head, &url, &[edit])?;
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
        assert!(ran.stdout.contains("repository_id"), "{}", ran.stdout);
        assert!(!rig.store().join("git").exists());
    }
    Ok(())
}

#[test]
fn checkout_declares_the_commit_and_ref_outputs() -> Result<()> {
    let rig = Rig::new()?;
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    git(&rig.root.join("work"), &["tag", "v1", &rig.head])?;
    git(
        &rig.root.join("work"),
        &["push", "-q", &upstream.to_string_lossy(), "refs/tags/v1"],
    )?;
    let queue = "refs/heads/gh-readonly-queue/main/pr-1";
    let branch = "refs/heads/main";
    let pull = "refs/pull/7/merge";
    for (fixture, sha, from, git_ref) in [
        ("checkout_outputs", &rig.head, branch, branch),
        ("checkout_outputs", &rig.head, branch, "refs/tags/v1"),
        ("checkout_outputs", &rig.head, branch, queue),
        ("checkout_pull_request_outputs", &rig.merge, pull, pull),
    ] {
        let job = rig.job_edit(fixture, sha, &rig.server_url(), &[(from, git_ref)])?;
        let scratch = Scratch::new()?;
        let ran = finish(rig.start(&job, &scratch)?)?;
        assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
        assert!(
            ran.has_line(&format!("outputs commit={sha} ref={git_ref}")),
            "{}",
            ran.stdout
        );
    }
    Ok(())
}

#[test]
fn branches_named_x_and_x_slash_y_both_fetch_their_commit_by_sha() -> Result<()> {
    let rig = Rig::new()?;
    let work = rig.root.join("work");
    let upstream = rig.root.join("up/tailrocks/velnor-new.git");
    let target = upstream.to_string_lossy().into_owned();
    git(
        &upstream,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    )?;
    git(&work, &["checkout", "-q", "--detach"])?;
    let mut forced = Vec::new();
    for name in ["forced-first", "forced-second"] {
        let sha = commit(&work, name)?;
        git(
            &work,
            &["push", "-q", &target, &format!("{sha}:refs/heads/queue")],
        )?;
        git(&work, &["push", "-q", &target, ":refs/heads/queue"])?;
        forced.push(sha);
    }
    git(&work, &["checkout", "-q", "main"])?;
    for (sha, branch) in forced.iter().zip(["X", "X/Y"]) {
        let event = format!(r#"{{"kind": "branch", "branch": "{branch}"}}"#);
        let facts = trusted_facts(&rig, 8, sha, &event)?;
        assert_eq!(
            checkout_trusted(&rig, &facts)?,
            (Some(0), sha.clone()),
            "{branch}"
        );
    }
    let refs = git(
        &rig.mirror_of(8),
        &["for-each-ref", "--format=%(refname)", "refs/velnor"],
    )?;
    let mut expected: Vec<String> = forced
        .iter()
        .map(|sha| format!("refs/velnor/sha/{sha}"))
        .collect();
    expected.sort();
    assert_eq!(refs.lines().collect::<Vec<_>>(), expected);
    Ok(())
}
