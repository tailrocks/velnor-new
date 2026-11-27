// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_checkout::{Rig, finish, git};
use crate::native_checkout_env::wrapper_env;
use crate::native_checkout_walk::{checks_out, warm};

const SETTINGS: [&str; 2] = ["http.lowSpeedLimit=1", "http.lowSpeedTime=600"];

fn lock_of(rig: &Rig) -> PathBuf {
    let mut path = rig.mirror().into_os_string();
    path.push(".lock");
    PathBuf::from(path)
}

fn is_locked(path: &Path) -> Result<bool> {
    Ok(matches!(
        File::open(path)?.try_lock(),
        Err(TryLockError::WouldBlock)
    ))
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_failed_job_leaves_the_marker_for_the_next_one() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let marker = rig.mirror().join("sweep-needed");
    assert!(!marker.exists(), "a clean job left the marker");
    let job = rig.job("checkout_depth1", &"a".repeat(40), &rig.server_url())?;
    let ran = finish(rig.start(&job, &Scratch::new()?)?)?;
    assert_ne!(ran.code, Some(0), "{}\n{}", ran.stdout, ran.stderr);
    assert!(marker.exists(), "a failed job cleared the marker");
    checks_out(&rig, &tip, 2)?;
    assert!(!marker.exists(), "a clean job left the marker");
    Ok(())
}

#[test]
fn a_git_that_outlives_its_killed_runner_keeps_the_mirror_lock() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let started = rig.root.join("fetching");
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" fetch \"*) touch '{}'; sleep 4;; esac\n",
            started.display()
        ),
    )?;
    let next = rig.advance("y")?;
    let job = rig.job("checkout_depth1", &next, &rig.server_url())?;
    let runner = rig.start(&job, &Scratch::new()?)?;
    wait_for(&started);
    let killed = Command::new("kill")
        .args(["-KILL", &runner.id().to_string()])
        .status()?;
    assert!(killed.success());
    finish(runner)?;
    let lock = lock_of(&rig);
    assert!(is_locked(&lock)?, "the lock went with the killed runner");
    let stray = rig.mirror().join("objects/ab/posing/inner");
    std::fs::create_dir_all(stray.parent().expect("parent"))?;
    std::fs::write(&stray, b"x")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while is_locked(&lock)? && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!is_locked(&lock)?, "the surviving git never ended");
    assert!(rig.mirror().join("sweep-needed").exists());
    wrapper_env(&rig.root, "")?;
    checks_out(&rig, &next, 2)?;
    assert!(!stray.exists(), "the next job did not sweep");
    assert_eq!(git(&rig.mirror(), &["rev-parse", "refs/heads/main"])?, next);
    Ok(())
}

fn plant(marker: &Path, outside: &Path, kind: &str) -> Result<()> {
    match kind {
        "directory" => std::fs::create_dir_all(marker.join("inner"))?,
        "link" => std::os::unix::fs::symlink(outside, marker)?,
        _ => assert!(Command::new("mkfifo").arg(marker).status()?.success()),
    }
    Ok(())
}

#[test]
fn a_marker_that_is_not_a_regular_file_is_replaced_and_the_job_succeeds() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    let marker = rig.mirror().join("sweep-needed");
    let outside = rig.root.join("outside-the-mirror");
    for kind in ["link", "directory", "fifo"] {
        plant(&marker, &outside, kind)?;
        let next = rig.advance(kind)?;
        checks_out(&rig, &next, 2)?;
        assert!(!marker.exists(), "the {kind} marker survived a clean job");
        assert!(!outside.exists(), "the {kind} marker was followed");
    }
    Ok(())
}

#[test]
fn every_broker_fetch_gives_up_on_a_stalled_transfer() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    std::fs::write(
        rig.mirror().join("refs/heads/main"),
        format!("{}\n", "1".repeat(40)),
    )?;
    wrapper_env(
        &rig.root,
        "case \" $* \" in *\" --refetch \"*) ;; *\" fetch \"*) exit 0;; esac\n",
    )?;
    let next = rig.advance("y")?;
    checks_out(&rig, &next, 2)?;
    let trace = std::fs::read_to_string(rig.trace())?;
    let fetches: Vec<&str> = trace
        .lines()
        .filter(|line| {
            line.contains("\"event\":\"start\"") && line.contains("--no-auto-maintenance")
        })
        .collect();
    assert!(
        fetches.iter().any(|line| line.contains("--refetch")),
        "the broker never refetched"
    );
    assert!(
        fetches.iter().any(|line| !line.contains("--refetch")),
        "the broker never fetched incrementally"
    );
    for line in fetches {
        for setting in SETTINGS {
            assert!(line.contains(setting), "a fetch lacks {setting}: {line}");
        }
    }
    Ok(())
}

#[test]
fn the_ref_update_of_a_refetch_keeps_the_mirror_lock_after_the_runner_dies() -> Result<()> {
    let rig = Rig::new()?;
    let tip = rig.advance("x")?;
    warm(&rig, &tip)?;
    std::fs::write(
        rig.mirror().join("refs/heads/main"),
        format!("{}\n", "1".repeat(40)),
    )?;
    let started = rig.root.join("updating");
    wrapper_env(
        &rig.root,
        &format!(
            "case \" $* \" in *\" --refetch \"*) ;; *\" fetch \"*) exit 0;; *\" update-ref --stdin \"*) touch '{}'; sleep 6;; esac\n",
            started.display()
        ),
    )?;
    let next = rig.advance("y")?;
    let job = rig.job("checkout_depth1", &next, &rig.server_url())?;
    let runner = rig.start(&job, &Scratch::new()?)?;
    wait_for(&started);
    assert!(started.exists(), "the refetch never updated a ref");
    let killed = Command::new("kill")
        .args(["-KILL", &runner.id().to_string()])
        .status()?;
    assert!(killed.success());
    finish(runner)?;
    let lock = lock_of(&rig);
    assert!(is_locked(&lock)?, "the ref update lost the mirror lock");
    let deadline = Instant::now() + Duration::from_secs(60);
    while is_locked(&lock)? && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!is_locked(&lock)?, "the surviving git never ended");
    Ok(())
}
