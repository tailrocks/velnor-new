// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use crate::run::scratch;

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
    velnor_ci::io::pump::pump(from, &mut shown, &log);
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

#[cfg(unix)]
mod tests_run {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};
    use std::time::{Duration, SystemTime};

    use super::scratch;

    const BOLD: &str = "\u{1b}[1m";
    const RESET: &str = "\u{1b}[0m";

    fn fake_nextest(directory: &Path, script: &str) {
        let program = directory.join("cargo-nextest");
        let mut body = String::from("#!/bin/sh\nif [ \"$2\" = list ]; then exit 1; fi\n");
        body.push_str(script);
        body.push('\n');
        fs::write(&program, body).unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn run_with_output(name: &str, script: &str, command: &[&str]) -> (Output, String, PathBuf) {
        let (output, written, temp, _) = execute_nextest(name, script, command, None);
        (output, written, temp)
    }

    fn execute_nextest(
        name: &str,
        script: &str,
        command: &[&str],
        grace_ms: Option<&str>,
    ) -> (Output, String, PathBuf, SystemTime) {
        let temp = scratch(name);
        fake_nextest(&temp, script);
        let outputs = temp.join("github-output");
        fs::write(&outputs, "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
        let cargo_home = temp.join("cargo-home");
        fs::create_dir_all(&cargo_home).unwrap_or_else(|e| unreachable!("{e}"));
        let path = format!(
            "{}:{}",
            temp.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        child
            .arg("run")
            .arg("--")
            .args(command)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", &outputs)
            .env("CARGO_HOME", &cargo_home)
            .env("PATH", path);
        if let Some(grace_ms) = grace_ms {
            child.env("VELNOR_CI_RUN_PUMP_GRACE_MS", grace_ms);
        }
        let output = child.output().unwrap_or_else(|e| unreachable!("{e}"));
        let returned = SystemTime::now();
        let written = fs::read_to_string(&outputs).unwrap_or_default();
        (output, written, temp, returned)
    }

    // Time velnor-ci spent after the fake nextest exited. The script touches
    // `exited` as its last act, so start-up under load does not count.
    fn after_exit(temp: &Path, returned: SystemTime) -> Duration {
        let exited = fs::metadata(temp.join("exited"))
            .and_then(|meta| meta.modified())
            .unwrap_or_else(|e| unreachable!("{e}"));
        returned.duration_since(exited).unwrap_or_default()
    }

    const NEXTEST_RUN: [&str; 6] = [
        "cargo-nextest",
        "nextest",
        "run",
        "--archive-file",
        "a.tar.zst",
        "--no-tests=pass",
    ];

    #[test]
    fn run_appends_the_number_of_tests_nextest_ran_to_the_step_output() {
        let split = format!(
            "{{ printf '     Summary [   0.020s] {BOLD}7'; sleep 0.3; printf '{RESET} tests run: 7 passed, 0 skipped\\n'; }} >&2"
        );
        let (output, written, _) = run_with_output("tests-run-split", &split, &NEXTEST_RUN);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(written, "existing=1\ntests_run=7\ntests_listed=7\n");
    }

    #[test]
    fn run_counts_failed_tests_and_keeps_the_exit_code_of_nextest() {
        let script = format!(
            "echo '     Summary [   1.200s] {BOLD}5{RESET} tests run: 3 passed, {BOLD}2{RESET} failed, 0 skipped' >&2; exit 100"
        );
        let (output, written, _) = run_with_output("tests-run-failed", &script, &NEXTEST_RUN);
        assert_eq!(output.status.code(), Some(100));
        assert_eq!(written, "existing=1\ntests_run=5\ntests_listed=5\n");
    }

    #[test]
    fn run_reports_a_run_without_tests_as_zero() {
        let script = "echo '     Summary [   0.001s] 0 tests run: 0 passed, 0 skipped' >&2";
        let (output, written, _) = run_with_output("tests-run-zero", script, &NEXTEST_RUN);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(written, "existing=1\ntests_run=0\ntests_listed=0\n");
    }

    #[test]
    fn run_reports_the_skipped_tests_as_listed_when_a_filter_ran_none() {
        let script = format!(
            "echo '     Summary [   0.001s] {BOLD}0{RESET} tests run: 0 passed, {BOLD}4{RESET} skipped' >&2"
        );
        let (output, written, _) = run_with_output("tests-run-filtered", &script, &NEXTEST_RUN);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(written, "existing=1\ntests_run=0\ntests_listed=4\n");
    }

    #[test]
    fn run_lists_run_plus_skipped_and_leaves_listed_out_without_a_skipped_count() {
        let mixed = "echo '     Summary [   0.5s] 3 tests run: 2 passed, 1 failed, 5 skipped' >&2";
        let (_, written, _) = run_with_output("tests-run-mixed", mixed, &NEXTEST_RUN);
        assert_eq!(written, "existing=1\ntests_run=3\ntests_listed=8\n");
        let bare = "echo '     Summary [   0.5s] 3 tests run: 3 passed' >&2";
        let (_, written, _) = run_with_output("tests-run-bare", bare, &NEXTEST_RUN);
        assert_eq!(written, "existing=1\ntests_run=3\n");
    }

    #[test]
    fn run_writes_no_count_for_other_commands_or_a_run_that_printed_no_summary() {
        let summary = "echo '     Summary [   0.020s] 9 tests run: 9 passed, 0 skipped' >&2";
        let cases: [(&str, &str, Vec<&str>); 4] = [
            ("list", summary, vec!["cargo-nextest", "nextest", "list"]),
            (
                "other",
                summary,
                vec![
                    "sh",
                    "-c",
                    "echo '     Summary [   0.020s] 9 tests run: 9 passed'",
                ],
            ),
            (
                "crash",
                "echo 'error: no archive' >&2; exit 1",
                NEXTEST_RUN.to_vec(),
            ),
            ("other-program", summary, vec!["echo", "nextest", "run"]),
        ];
        for (name, script, command) in cases {
            let (_, written, _) =
                run_with_output(&format!("tests-run-no-{name}"), script, &command);
            assert_eq!(written, "existing=1\n", "{name}");
        }
    }

    #[test]
    fn run_withholds_counts_when_a_descendant_keeps_the_summary_pipe_open() {
        // Output stays on the pipe: the background `sleep` holds the summary pipe,
        // and that is the behaviour under test.
        let script = "echo '     Summary [   0.001s] 3 tests run: 3 passed, 0 skipped' >&2\n\
sleep 30 &\n\
touch \"$RUNNER_TEMP/exited\"\n\
exit 0";
        let (output, written, temp, returned) =
            execute_nextest("tests-run-held", script, &NEXTEST_RUN, Some("300"));
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(written, "existing=1\n");
        let elapsed = after_exit(&temp, returned);
        assert!(elapsed < Duration::from_secs(15), "{elapsed:?}");
    }

    #[test]
    fn run_writes_no_counts_when_a_held_pipe_never_printed_a_summary() {
        let script = "sleep 30 &\ntouch \"$RUNNER_TEMP/exited\"\nexit 9";
        let (output, written, temp, returned) =
            execute_nextest("tests-run-held-empty", script, &NEXTEST_RUN, Some("300"));
        assert_eq!(output.status.code(), Some(9), "{output:?}");
        assert_eq!(written, "existing=1\n");
        let elapsed = after_exit(&temp, returned);
        assert!(elapsed < Duration::from_secs(15), "{elapsed:?}");
    }

    #[test]
    fn run_does_not_need_a_step_output_file() {
        let temp = scratch("tests-run-no-output");
        fake_nextest(
            &temp,
            "echo '     Summary [ 0.1s] 2 tests run: 2 passed' >&2",
        );
        let path = format!(
            "{}:{}",
            temp.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .arg("run")
            .arg("--")
            .args(NEXTEST_RUN)
            .env("RUNNER_TEMP", &temp)
            .env_remove("GITHUB_OUTPUT")
            .env("PATH", path)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
    }
}
