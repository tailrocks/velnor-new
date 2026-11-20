// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! No outcome of a cache-swap style test may leave processes blocked on a FIFO.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::install_guard::Running;
use crate::setup_action::{Install, TestResult, program};

const VICTIM_MARKER: &str = "VELNOR_ORPHAN_VICTIM_MARKER";
const VICTIM: &str = "setup_action_orphans::the_victim_of_an_external_kill";

/// Command lines of every live process that mentions `dir`.
fn processes_of(dir: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let listing = Command::new(program("ps"))
        .args(["-axww", "-o", "pid=,command="])
        .output()?;
    let needle = dir.display().to_string();
    Ok(String::from_utf8(listing.stdout)?
        .lines()
        .filter(|line| line.contains(&needle))
        .map(str::to_owned)
        .collect())
}

fn until_gone(dir: &Path, limit: Duration) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + limit;
    loop {
        let left = processes_of(dir)?;
        if left.is_empty() || Instant::now() >= deadline {
            return Ok(left);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A `bash` script that, like the real install, forks `cp` and `head` onto a FIFO
/// nobody ever writes.
fn blocked_on_a_fifo(install: &Install) -> Result<Command, Box<dyn std::error::Error>> {
    let fifo = install.work.join("entry.fifo");
    assert!(
        Command::new(program("mkfifo"))
            .arg(&fifo)
            .status()?
            .success()
    );
    let mut command = Command::new(program("bash"));
    command
        .args([
            "-c",
            "cp \"$1\" /dev/null & head -c1 \"$1\"; wait",
            "install.sh",
        ])
        .arg(&fifo)
        .stdout(Stdio::null());
    Ok(command)
}

fn started(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while processes_of(dir)?.len() < 3 {
        if Instant::now() >= deadline {
            return Err(format!(
                "the blocked readers never started: {:?}",
                processes_of(dir)?
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

#[test]
fn dropping_the_guard_mid_run_leaves_no_install_cp_or_head_behind() -> TestResult {
    let install = Install::new("orphan-drop")?;
    let command = blocked_on_a_fifo(&install)?;
    let running = Running::start(command, &install.work, Duration::from_secs(60))?;
    started(&install.work)?;
    drop(running);
    let left = until_gone(&install.work, Duration::from_secs(10))?;
    assert!(left.is_empty(), "survivors after drop: {left:?}");
    Ok(())
}

#[test]
fn a_hung_install_is_killed_at_its_deadline_and_reported_as_a_hang() -> TestResult {
    let install = Install::new("orphan-deadline")?;
    let command = blocked_on_a_fifo(&install)?;
    let mut running = Running::start(command, &install.work, Duration::from_secs(60))?;
    started(&install.work)?;
    assert!(running.wait_within(Duration::from_millis(300))?.is_none());
    drop(running);
    let left = until_gone(&install.work, Duration::from_secs(10))?;
    assert!(left.is_empty(), "survivors after the deadline: {left:?}");
    Ok(())
}

/// Child half of the test below: only runs when the parent asks for it, then
/// waits to be killed without ever reaching a `Drop`.
#[test]
fn the_victim_of_an_external_kill() -> TestResult {
    let Some(marker) = std::env::var_os(VICTIM_MARKER) else {
        return Ok(());
    };
    let install = Install::new("orphan-victim")?;
    let command = blocked_on_a_fifo(&install)?;
    let _running = Running::start(command, &install.work, Duration::from_secs(4))?;
    started(&install.work)?;
    fs::write(&marker, install.work.display().to_string())?;
    std::thread::sleep(Duration::from_secs(120));
    Ok(())
}

#[test]
fn killing_the_test_process_leaves_nothing_behind_once_the_watchdog_fires() -> TestResult {
    let scratch = Install::new("orphan-parent")?;
    let marker = scratch.work.join("victim-work-dir");
    let mut victim = Command::new(std::env::current_exe()?)
        .args([VICTIM, "--exact", "--test-threads=1"])
        .env(VICTIM_MARKER, &marker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() {
        if Instant::now() >= deadline {
            victim.kill().ok();
            victim.wait().ok();
            return Err("the victim never started its install".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let work = std::path::PathBuf::from(fs::read_to_string(&marker)?);
    let alive = processes_of(&work)?;
    assert!(alive.len() >= 3, "{alive:?}");
    victim.kill()?;
    victim.wait()?;
    let left = until_gone(&work, Duration::from_secs(30))?;
    assert!(left.is_empty(), "survivors after SIGKILL: {left:?}");
    assert!(!work.exists(), "the temp dir leaked: {}", work.display());
    Ok(())
}
