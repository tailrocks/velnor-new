// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `run` stops its environment on every way a user or a supervisor ends it, not on Ctrl-C alone:
//! `timeout`, a CI job's cancel and `kill` send SIGTERM.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::support::temp_dir;

/// A `docker` that records every call and blocks on `network create`, where `up` is waiting.
const DOCKER: &str = "#!/bin/sh\necho \"$*\" >> \"$DOCKER_LOG\"\n[ \"$1 $2\" = \"network create\" ] && sleep 120\nexit 0\n";

fn run(dir: &Path, bin: &Path, log: &Path) -> Child {
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(["-C", &dir.join("repo").display().to_string()])
            .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
            .args(args)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "git {args:?}");
    };
    std::fs::create_dir_all(dir.join("repo/.github/workflows"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(
        dir.join("repo/.github/workflows/ci.yml"),
        "name: ci\non: push\njobs: {}\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "workflow"]);
    std::fs::write(
        dir.join("signal.json"),
        r#"{"description":"a push","steps":[{"push":"{default}"}]}"#,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    Command::new(env!("CARGO_BIN_EXE_velnor-local"))
        .args(["run", "--runner", "plain", "--scenarios"])
        .arg(dir)
        .arg("signal")
        .arg("--repo")
        .arg(dir.join("repo"))
        .arg("--dir")
        .arg(dir.join("state"))
        .env("PATH", path)
        .env("DOCKER_LOG", log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn interrupted_by(signal: &str) -> (Option<i32>, Vec<String>) {
    let dir = temp_dir(&format!("run-signal-{signal}"));
    let bin = dir.join("bin");
    let log = dir.join("docker.log");
    std::fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(bin.join("docker"), DOCKER).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(bin.join("docker"), std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut child = run(&dir, &bin, &log);
    wait_until("the environment to start", || {
        calls(&log)
            .iter()
            .any(|call| call.starts_with("network create"))
    });
    let sent = Command::new("kill")
        .args([&format!("-{signal}"), &child.id().to_string()])
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(sent.success());
    let mut status = None;
    wait_until("run to exit", || {
        status = child.try_wait().unwrap_or_else(|e| unreachable!("{e}"));
        status.is_some()
    });
    let all = calls(&log);
    let started = all
        .iter()
        .position(|call| call.starts_with("network create"))
        .unwrap_or_else(|| unreachable!("no network create"));
    let after = all[started + 1..].to_vec();
    std::fs::remove_dir_all(&dir).ok();
    (status.and_then(|status| status.code()), after)
}

#[test]
fn sigterm_removes_the_environment_and_exits_with_a_failure() {
    let (code, after) = interrupted_by("TERM");
    assert!(
        code.is_some_and(|code| code != 0),
        "exit {code:?}: SIGTERM killed run outright"
    );
    assert!(
        after.iter().any(|call| call.starts_with("ps -aq"))
            && after.iter().any(|call| call.starts_with("network ls")),
        "no teardown after SIGTERM: {after:?}"
    );
}

#[test]
fn sigint_removes_the_environment_and_exits_with_a_failure() {
    let (code, after) = interrupted_by("INT");
    assert!(code.is_some_and(|code| code != 0), "exit {code:?}");
    assert!(
        after.iter().any(|call| call.starts_with("network ls")),
        "no teardown: {after:?}"
    );
}
