// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, git};
use crate::native_checkout_repair::{object_path, overwrite};
use crate::native_checkout_walk::{import, publish, started};

const VERIFIED: &str = "--verify-objects";

fn unwritable_commit(rig: &Rig) -> Result<String> {
    let stream = format!(
        "commit refs/heads/main\ncommitter t <t@t> 1700000100 +0000\ndata 1\nx\nfrom {}\nM 644 inline {}\ndata 1\nz\n\n",
        rig.head,
        "n".repeat(300)
    );
    import(rig, &stream)?;
    publish(rig)?;
    git(&rig.root.join("work"), &["rev-parse", "main"])
}

fn fails_to_check_out(rig: &Rig, fixture: &str, sha: &str) -> Result<()> {
    let ran = rig.run(fixture, sha, &Scratch::new()?)?;
    assert_eq!(ran.code, Some(1), "{}\n{}", ran.stdout, ran.stderr);
    assert!(
        ran.stdout.contains("git checkout failed"),
        "{}\n{}",
        ran.stdout,
        ran.stderr
    );
    Ok(())
}

#[test]
fn a_repeated_failure_on_a_sound_mirror_is_not_scanned_again() -> Result<()> {
    let rig = Rig::new()?;
    let sha = unwritable_commit(&rig)?;
    for _ in 0..3 {
        fails_to_check_out(&rig, "checkout_depth1", &sha)?;
    }
    assert_eq!(started(&rig, VERIFIED), 1);
    assert_eq!(rig.refetches(), 0);
    Ok(())
}

#[test]
fn a_sound_result_covers_only_the_window_it_checked() -> Result<()> {
    let rig = Rig::new()?;
    let sha = unwritable_commit(&rig)?;
    fails_to_check_out(&rig, "checkout_depth1", &sha)?;
    fails_to_check_out(&rig, "checkout_depth2", &sha)?;
    assert_eq!(started(&rig, VERIFIED), 2);
    fails_to_check_out(&rig, "checkout_depth2", &sha)?;
    assert_eq!(started(&rig, VERIFIED), 2);
    Ok(())
}

#[test]
fn a_sound_result_expires_after_a_day() -> Result<()> {
    let rig = Rig::new()?;
    let sha = unwritable_commit(&rig)?;
    fails_to_check_out(&rig, "checkout_depth1", &sha)?;
    let marker = rig.mirror().join("sound").join(format!("{sha}-last-1"));
    let recorded = std::fs::File::options().write(true).open(&marker)?;
    recorded.set_modified(SystemTime::now() - Duration::from_hours(23))?;
    fails_to_check_out(&rig, "checkout_depth1", &sha)?;
    assert_eq!(started(&rig, VERIFIED), 1);
    recorded.set_modified(SystemTime::now() - Duration::from_hours(25))?;
    fails_to_check_out(&rig, "checkout_depth1", &sha)?;
    assert_eq!(started(&rig, VERIFIED), 2);
    Ok(())
}

#[test]
fn a_stale_shallow_file_in_the_mirror_is_removed() -> Result<()> {
    let rig = Rig::new()?;
    let warm = rig.run("checkout_depth2", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    let shallow = rig.mirror().join("shallow");
    std::fs::write(&shallow, format!("{}\n", rig.head))?;
    let scratch = Scratch::new()?;
    let ran = rig.run("checkout_depth2", &rig.head, &scratch)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert!(!shallow.exists(), "the mirror kept its shallow file");
    assert_eq!(
        git(&scratch.workspace(), &["rev-list", "--count", "HEAD"])?,
        "2"
    );
    Ok(())
}

#[test]
fn a_repair_drops_the_branches_the_server_no_longer_has() -> Result<()> {
    let rig = Rig::new()?;
    let upstream = rig
        .root
        .join("up/tailrocks/velnor-new.git")
        .to_string_lossy()
        .into_owned();
    let work = rig.root.join("work");
    git(&work, &["push", "-q", &upstream, "feature:gone"])?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    git(
        &rig.mirror(),
        &["rev-parse", "--verify", "-q", "refs/heads/gone"],
    )?;
    git(&work, &["push", "-q", &upstream, ":gone"])?;
    let blob = git(
        &rig.mirror(),
        &["rev-parse", &format!("{}:second", rig.head)],
    )?;
    overwrite(&object_path(&rig.mirror(), &blob), b"")?;
    let ran = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(rig.refetches(), 1);
    assert!(
        git(
            &rig.mirror(),
            &["rev-parse", "--verify", "-q", "refs/heads/gone"]
        )
        .is_err(),
        "the mirror kept a branch the server deleted"
    );
    git(
        &rig.mirror(),
        &["rev-parse", "--verify", "-q", "refs/heads/main"],
    )?;
    Ok(())
}

#[test]
fn a_repair_replaces_a_branch_by_one_under_its_name() -> Result<()> {
    let rig = Rig::new()?;
    let upstream = rig
        .root
        .join("up/tailrocks/velnor-new.git")
        .to_string_lossy()
        .into_owned();
    let work = rig.root.join("work");
    git(&work, &["push", "-q", &upstream, "feature:topic"])?;
    let warm = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(warm.code, Some(0), "{}\n{}", warm.stdout, warm.stderr);
    git(
        &work,
        &["push", "-q", &upstream, ":topic", "feature:topic/part"],
    )?;
    let blob = git(
        &rig.mirror(),
        &["rev-parse", &format!("{}:second", rig.head)],
    )?;
    overwrite(&object_path(&rig.mirror(), &blob), b"")?;
    let ran = rig.run("checkout_depth1", &rig.head, &Scratch::new()?)?;
    assert_eq!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert_eq!(rig.refetches(), 1);
    git(
        &rig.mirror(),
        &["rev-parse", "--verify", "-q", "refs/heads/topic/part"],
    )?;
    Ok(())
}
