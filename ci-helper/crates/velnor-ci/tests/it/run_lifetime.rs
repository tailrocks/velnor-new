// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use crate::run::{run_with_grace, scratch};

#[test]
fn run_returns_when_the_command_exits_although_a_background_process_keeps_its_pipes_open() {
    let temp = scratch("background");
    let started = std::time::Instant::now();
    let output = run_with_grace(
        &temp,
        "200",
        &["sh", "-c", "sleep 30 & echo started; exit 4"],
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "started\n");
}

#[test]
fn run_waits_for_all_open_pipes_within_one_shared_grace_period() {
    let temp = scratch("shared-grace");
    let started = std::time::Instant::now();
    let output = run_with_grace(
        &temp,
        "1000",
        &["sh", "-c", "sleep 30 & echo out; echo err >&2; exit 5"],
    );
    let elapsed = started.elapsed();
    assert_eq!(output.status.code(), Some(5), "{output:?}");
    assert!(
        elapsed >= std::time::Duration::from_millis(900),
        "{elapsed:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(1900),
        "{elapsed:?}"
    );
}

struct Flaky {
    chunks: Vec<std::io::Result<Vec<u8>>>,
}

impl std::io::Read for Flaky {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.chunks.is_empty() {
            return Ok(0);
        }
        self.chunks.remove(0).map(|bytes| {
            buffer[..bytes.len()].copy_from_slice(&bytes);
            bytes.len()
        })
    }
}

#[test]
fn run_keeps_reading_after_an_interrupted_read() {
    let from = Flaky {
        chunks: vec![
            Ok(b"before ".to_vec()),
            Err(std::io::ErrorKind::Interrupted.into()),
            Ok(b"after".to_vec()),
        ],
    };
    let mut shown = Vec::new();
    let log = std::sync::Mutex::new(Vec::new());
    velnor_ci::stream::pump(from, &mut shown, &log);
    assert_eq!(shown, b"before after");
    assert_eq!(
        log.into_inner().unwrap_or_else(|e| unreachable!("{e}")),
        b"before after"
    );
}

#[cfg(unix)]
#[test]
fn run_keeps_the_exit_code_of_the_command_when_the_final_log_line_cannot_be_written() {
    let temp = scratch("full-log");
    let script = format!(
        "trap '' XFSZ; ulimit -f 1; exec {} run -- sh -c 'head -c 2000 /dev/zero | tr \"\\0\" x; exit 7'",
        env!("CARGO_BIN_EXE_velnor-ci")
    );
    let output = Command::new("sh")
        .args(["-c", &script])
        .env("RUNNER_TEMP", &temp)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(7), "{output:?}");
    assert_eq!(output.stdout.len(), 2000);
    assert!(String::from_utf8_lossy(&output.stderr).contains("velnor-run.log"));
}

#[cfg(unix)]
#[test]
fn run_forwards_sigterm_to_the_command_and_exits_with_its_code() {
    forwards("TERM", 5);
}

#[cfg(unix)]
#[test]
fn run_forwards_sigint_to_the_command_and_exits_with_its_code() {
    forwards("INT", 6);
}

#[cfg(unix)]
fn forwards(signal: &str, code: i32) {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let temp = scratch(&format!("sig{signal}"));
    let script = format!(
        "trap 'echo got-{signal}; exit {code}' {signal}; echo ready; i=0; while [ $i -lt 100 ]; do sleep 0.1; i=$((i+1)); done; exit 9"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "sh", "-c", &script])
        .env("RUNNER_TEMP", &temp)
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = child.stdout.take().unwrap_or_else(|| unreachable!("piped"));
    let mut lines = BufReader::new(stdout).lines();
    assert_eq!(lines.next().and_then(Result::ok).as_deref(), Some("ready"));
    let sent = Command::new("kill")
        .args([&format!("-{signal}"), &child.id().to_string()])
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(sent.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap_or_else(|e| unreachable!("{e}")) {
            break Some(status);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    let status = status.unwrap_or_else(|| {
        let pid = i32::try_from(child.id()).unwrap_or_else(|e| unreachable!("{e}"));
        killpg(Pid::from_raw(pid), Signal::SIGKILL).ok();
        child.wait().unwrap_or_else(|e| unreachable!("{e}"))
    });
    assert_eq!(status.code(), Some(code));
    assert_eq!(
        lines.next().and_then(Result::ok).as_deref(),
        Some(format!("got-{signal}").as_str())
    );
}
