// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use velnor_test_support::{real_cargo, real_cargo_str};

pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = crate::scratch_path("velnor-ci-run", name);
    if dir.exists() {
        fs::remove_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
    dir
}

pub(crate) fn run(temp: &Path, command: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", temp)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn run_with_grace(temp: &Path, millis: &str, command: &[&str]) -> Output {
    // The witnesses scan CARGO_HOME after the command exits; an empty one keeps
    // that scan of the developer's cache out of the timing.
    let cargo_home = temp.join("cargo-home");
    fs::create_dir_all(&cargo_home).unwrap_or_else(|e| unreachable!("{e}"));
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(command)
        .env("RUNNER_TEMP", temp)
        .env("CARGO_HOME", cargo_home)
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", millis)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn log(temp: &Path) -> String {
    fs::read_to_string(temp.join("velnor-run.log")).unwrap_or_default()
}

#[test]
fn run_exits_with_the_exit_code_of_the_command() {
    let temp = scratch("exit");
    assert_eq!(run(&temp, &["false"]).status.code(), Some(1));
    assert_eq!(run(&temp, &["true"]).status.code(), Some(0));
    assert_eq!(run(&temp, &["sh", "-c", "exit 7"]).status.code(), Some(7));
    assert_eq!(
        run(&temp, &["sh", "-c", "kill -TERM $$"]).status.code(),
        Some(143)
    );
}

#[test]
fn run_prints_cargos_version_unchanged_and_appends_it_to_the_log() {
    let temp = scratch("cargo");
    let cargo = real_cargo();
    let output = run(&temp, &[&real_cargo_str(), "--version"]);
    let version = Command::new(&cargo)
        .arg("--version")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert!(version.stdout.starts_with(b"cargo "));
    assert_eq!(output.stdout, version.stdout);
    assert!(log(&temp).contains(String::from_utf8_lossy(&version.stdout).trim()));
}

#[test]
fn run_keeps_each_stream_and_every_byte_and_appends_one_record_per_command() {
    let temp = scratch("streams");
    let big = "x".repeat(100_000);
    let script = format!("printf 'no newline'; printf 'warn' >&2; printf '{big}'");
    let first = run(&temp, &["sh", "-c", &script]);
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        format!("no newline{big}")
    );
    assert_eq!(first.stderr, b"warn");
    run(&temp, &["sh", "-c", "exit 3"]);
    let text = log(&temp);
    assert_eq!(
        text.matches("velnor-ci run: sh (2 arguments)").count(),
        2,
        "{}",
        text.len()
    );
    assert!(text.contains("no newline"));
    assert!(text.ends_with("velnor-ci run: exit 3\n"), "{text}");
    assert!(text.contains("velnor-ci run: exit 0\n"));
}

#[test]
fn run_names_a_program_it_cannot_start_and_exits_127() {
    let temp = scratch("missing");
    let output = run(&temp, &["velnor-no-such-program"]);
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("velnor-no-such-program"));
}

#[test]
fn run_does_not_start_the_command_when_the_log_cannot_be_written() {
    let temp = scratch("unwritable").join("missing-folder");
    let marker = scratch("unwritable-marker").join("started");
    let output = run(&temp, &["touch", marker.to_string_lossy().as_ref()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists());
}

#[test]
fn run_without_a_command_is_a_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn run_passes_every_argument_after_the_first_separator_to_the_command() {
    let temp = scratch("separator");
    let output = run(&temp, &["echo", "clippy", "--", "-D", "warnings", "--help"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "clippy -- -D warnings --help\n"
    );
}

#[test]
fn run_passes_shell_metacharacters_to_the_command_as_plain_arguments() {
    let temp = scratch("metacharacters");
    let marker = temp.join("touched");
    let marker = marker.to_string_lossy();
    let semicolon = format!("; touch {marker}");
    let substitution = format!("$(touch {marker})");
    let backticks = format!("`touch {marker}`");
    let quotes = "\"double\" 'single' \\ * ? | & > <";
    let arguments = [
        "printf",
        "[%s]\n",
        &semicolon,
        &substitution,
        &backticks,
        quotes,
    ];
    let output = run(&temp, &arguments);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("[{semicolon}]\n[{substitution}]\n[{backticks}]\n[{quotes}]\n")
    );
    assert!(!temp.join("touched").exists());
}

#[test]
fn run_logs_the_program_and_the_argument_count_but_never_the_arguments() {
    let temp = scratch("redaction");
    let output = run(&temp, &["true", "token-abc123", "--password=hunter2"]);
    assert!(output.status.success(), "{output:?}");
    let text = log(&temp);
    assert!(
        text.contains("velnor-ci run: true (2 arguments)\n"),
        "{text}"
    );
    assert!(
        !text.contains("token-abc123") && !text.contains("hunter2"),
        "{text}"
    );
}

#[test]
fn run_fails_without_starting_the_command_when_runner_temp_is_unset() {
    let marker = scratch("no-temp").join("started");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "touch"])
        .arg(&marker)
        .env_remove("RUNNER_TEMP")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("RUNNER_TEMP"));
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn run_refuses_a_log_that_is_a_symlink_and_leaves_its_target_alone() {
    let temp = scratch("symlink");
    let target = scratch("symlink-target").join("victim");
    fs::write(&target, "kept").unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&target, temp.join("velnor-run.log"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let marker = temp.join("started");
    let output = run(&temp, &["touch", marker.to_string_lossy().as_ref()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists());
    assert_eq!(fs::read_to_string(&target).unwrap_or_default(), "kept");
}

#[test]
fn run_fails_when_the_log_cannot_take_the_output() {
    let temp = scratch("log-full");
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "trap '' XFSZ; ulimit -f 1; exec {} run -- sh -c 'yes x | head -c 100000'",
            env!("CARGO_BIN_EXE_velnor-ci")
        ))
        .env("RUNNER_TEMP", &temp)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot write"),
        "{output:?}"
    );
}

#[test]
fn run_returns_when_the_command_exits_although_a_background_process_keeps_its_pipes_open() {
    let temp = scratch("background");
    // The shell marks its exit with a file, so the bound measures only how long
    // velnor-ci takes to return after the command exits, not process start-up under
    // load. Stdout stays a pipe: the background `sleep` holding the pipe is the
    // behaviour under test.
    let exited = temp.join("exited");
    let script = format!(
        "sleep 30 & echo started; touch '{}'; exit 4",
        exited.display()
    );
    let output = run_with_grace(&temp, "200", &["sh", "-c", &script]);
    let returned = std::time::SystemTime::now();
    let exited_at = fs::metadata(&exited)
        .and_then(|meta| meta.modified())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let after_exit = returned.duration_since(exited_at).unwrap_or_default();
    assert!(
        after_exit < std::time::Duration::from_secs(20),
        "{after_exit:?}"
    );
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "started\n");
}

#[test]
fn run_waits_for_all_open_pipes_within_one_shared_grace_period() {
    // Stdout and stderr both stay open after the command exits. One grace period
    // covers both, so the wait is about GRACE. A grace restarted per pipe would
    // take PIPES * GRACE. GRACE is large so that the slack above it, which absorbs
    // process startup under machine load (seconds at peak), never hides a per-pipe
    // wait.
    const PIPES: u32 = 2;
    const GRACE: std::time::Duration = std::time::Duration::from_millis(5000);
    let temp = scratch("shared-grace");
    // The shell marks its own exit with a file, so the test measures only the time
    // velnor-ci spends after the command exits. Start-up under load does not count.
    // Output goes to files, not pipes: a pipe reaches EOF only when every process
    // holding its write end exits, and a background `sleep` from a parallel test
    // thread can hold one.
    let exited = temp.join("exited");
    let script = format!(
        "sleep 30 & echo out; echo err >&2; touch '{}'; exit 5",
        exited.display()
    );
    let stdout = fs::File::create(temp.join("stdout")).unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = fs::File::create(temp.join("stderr")).unwrap_or_else(|e| unreachable!("{e}"));
    let status = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("run")
        .arg("--")
        .args(["sh", "-c", &script])
        .env("RUNNER_TEMP", &temp)
        // The witnesses scan CARGO_HOME after the command exits; an empty one keeps
        // that scan (of the developer's cache) out of the grace being measured.
        .env("CARGO_HOME", temp.join("cargo-home"))
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", GRACE.as_millis().to_string())
        .stdout(stdout)
        .stderr(stderr)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let returned = std::time::SystemTime::now();
    assert_eq!(status.code(), Some(5), "{status:?}");
    let exited_at = fs::metadata(&exited)
        .and_then(|meta| meta.modified())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let after_exit = returned.duration_since(exited_at).unwrap_or_default();
    assert!(after_exit >= GRACE, "{after_exit:?}");
    assert!(after_exit < GRACE * PIPES, "{after_exit:?}");
}

#[test]
fn the_child_gets_its_own_group_only_when_no_terminal_is_present() {
    assert!(velnor_ci::stream::separate_group(false));
    assert!(!velnor_ci::stream::separate_group(true));
}
