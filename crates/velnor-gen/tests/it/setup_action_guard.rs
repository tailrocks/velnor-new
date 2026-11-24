// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The guard's kill snippets run under the shell `sh` is on Linux (`dash`), whose
//! `kill` builtin rejects `kill -KILL -- -PGID`; a snippet that fails there kills
//! nothing, and the guard then waits out its whole hard limit.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use crate::install_guard::{KILL_GROUP, Running, WATCHDOG};
use crate::setup_action::{TestResult, program};

/// The strictest POSIX shell this host has: `dash` where it exists, else `sh`.
fn strict_shell() -> String {
    ["/bin/dash", "/usr/bin/dash"]
        .into_iter()
        .find(|path| Path::new(path).exists())
        .map_or_else(|| program("sh"), str::to_owned)
}

/// A `sleep` that leads its own process group, as the install script does.
fn sleeper(seconds: &str) -> Result<Child, Box<dyn std::error::Error>> {
    Ok(Command::new(program("sleep"))
        .arg(seconds)
        .process_group(0)
        .spawn()?)
}

fn dies_within(child: &mut Child, limit: Duration) -> Result<bool, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.kill().ok();
    child.wait().ok();
    Ok(false)
}

#[test]
fn the_group_kill_snippet_kills_the_group_under_the_strictest_shell() -> TestResult {
    let mut victim = sleeper("30")?;
    let status = Command::new(strict_shell())
        .args(["-c", KILL_GROUP, "sh"])
        .arg(victim.id().to_string())
        .status()?;
    let dead = dies_within(&mut victim, Duration::from_secs(5))?;
    assert!(dead && status.success(), "dead: {dead}, status: {status}");
    Ok(())
}

#[test]
fn the_watchdog_snippet_kills_the_group_and_removes_the_directory_under_the_strictest_shell()
-> TestResult {
    let mut victim = sleeper("30")?;
    let work = std::env::temp_dir().join(format!("velnor-gen-watchdog-{}", crate::io::unique_id()));
    std::fs::create_dir_all(&work)?;
    let mut watchdog = Command::new(strict_shell())
        .args(["-c", WATCHDOG, "sh"])
        .arg(victim.id().to_string())
        .arg(&work)
        .arg("1")
        .env("PATH", "/usr/bin:/bin")
        .spawn()?;
    let dead = dies_within(&mut victim, Duration::from_secs(8))?;
    watchdog.wait()?;
    let removed = !work.exists();
    std::fs::remove_dir_all(&work).ok();
    assert!(dead, "the watchdog left its group alive");
    assert!(removed, "the watchdog left the directory behind");
    Ok(())
}

#[test]
fn dropping_a_finished_run_does_not_wait_for_the_watchdog_deadline() -> TestResult {
    let work = std::env::temp_dir().join(format!("velnor-gen-guard-{}", crate::io::unique_id()));
    std::fs::create_dir_all(&work)?;
    let mut running = Running::start(
        Command::new(program("true")),
        &work,
        Duration::from_secs(30),
    )?;
    assert!(running.wait()?.success());
    let started = Instant::now();
    drop(running);
    let waited = started.elapsed();
    std::fs::remove_dir_all(&work).ok();
    assert!(waited < Duration::from_secs(10), "drop took {waited:?}");
    Ok(())
}
