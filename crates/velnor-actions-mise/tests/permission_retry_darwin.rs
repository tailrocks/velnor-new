// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use rustix::process::{Pid, Signal, kill_process_group};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[test]
fn anchored_transient_permission_reaches_public_receipt() -> Result<(), Box<dyn std::error::Error>>
{
    if in_fixture_child() {
        let command = shell("printf ready")?;
        let (output, _receipt) =
            command.run_quiescent_until(32, deadline(Duration::from_secs(2))?)?;
        assert!(output.success);
        assert_eq!(output.stdout, b"ready");
        return Ok(());
    }
    run_fixture(
        "anchored_transient_permission_reaches_public_receipt",
        "once",
        |log| {
            assert!(
                log.contains("errno=1"),
                "fixture did not inject EPERM: {log}"
            );
            assert!(log.contains("errno=3"), "real SRCH was not observed: {log}");
        },
    )
}

#[test]
fn anchored_persistent_permission_returns_error_without_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let error = shell("printf ready")?
            .run_quiescent_until(32, deadline(Duration::from_secs(2))?)
            .err()
            .ok_or_else(|| io::Error::other("persistent EPERM returned a receipt"))?;
        assert!(
            error
                .to_string()
                .contains("observe_group:Operation not permitted")
        );
        return Ok(());
    }
    run_fixture(
        "anchored_persistent_permission_returns_error_without_receipt",
        "persistent",
        |log| {
            assert!(
                log.contains("errno=1"),
                "fixture did not inject EPERM: {log}"
            );
        },
    )
}

#[test]
fn anchored_unrelated_observation_error_returns_immediately()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let error = shell("printf ready")?
            .run_quiescent_until(32, deadline(Duration::from_secs(2))?)
            .err()
            .ok_or_else(|| io::Error::other("EINVAL was retried as success"))?;
        assert!(error.to_string().contains("observe_group:Invalid argument"));
        return Ok(());
    }
    run_fixture(
        "anchored_unrelated_observation_error_returns_immediately",
        "other_once",
        |log| {
            assert!(
                log.contains("errno=22"),
                "fixture did not inject EINVAL: {log}"
            );
            assert!(
                !log.contains("errno=3"),
                "unrelated error was followed by observation: {log}"
            );
        },
    )
}

#[test]
fn unanchored_cap_cleanup_retries_permission_and_stops_descendant()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let directory = PathBuf::from(
            std::env::var_os("VELNOR_GROUP_PROBE_DIRECTORY")
                .ok_or_else(|| io::Error::other("fixture directory is missing"))?,
        );
        let ready = directory.join("ready");
        let release = directory.join("release");
        let continued = directory.join("continued");
        let _release_guard = ReleaseOnDrop(release.clone());
        let script = concat!(
            "(touch \"$1\"; attempt=0; while [ \"$attempt\" -lt 400 ]; do ",
            "[ -f \"$2\" ] && break; sleep 0.01; attempt=$((attempt + 1)); ",
            "done; [ -f \"$2\" ] || exit 1; touch \"$3\") & ",
            "attempt=0; while [ \"$attempt\" -lt 400 ]; do ",
            "[ -f \"$1\" ] && break; sleep 0.01; attempt=$((attempt + 1)); ",
            "done; [ -f \"$1\" ] || exit 1; printf overflow"
        );
        let command = shell_with_args(script, &[&ready, &release, &continued])?;
        let error = command
            .run_bounded(1, Duration::from_secs(3))
            .err()
            .ok_or_else(|| io::Error::other("stdout cap did not fail"))?;
        let message = error.to_string();
        assert!(message.contains("stdout_limit_exceeded:1"), "{message}");
        assert!(!message.contains("cleanup_failed:"), "{message}");
        assert!(ready.exists(), "descendant readiness was not observed");
        fs::write(release, b"release")?;
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until && !continued.exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!continued.exists(), "descendant continued after cleanup");
        return Ok(());
    }
    run_fixture(
        "unanchored_cap_cleanup_retries_permission_and_stops_descendant",
        "once",
        |log| {
            assert!(
                log.contains("errno=1"),
                "fixture did not inject EPERM: {log}"
            );
            assert!(log.contains("errno=3"), "real SRCH was not observed: {log}");
        },
    )
}

#[test]
fn anchored_present_group_exhausts_deadline_without_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let error = shell("printf ready")?
            .run_quiescent_until(32, deadline(Duration::from_millis(300))?)
            .err()
            .ok_or_else(|| io::Error::other("present group returned a receipt"))?;
        assert!(error.to_string().contains("group_drain_deadline_exhausted"));
        return Ok(());
    }
    run_fixture(
        "anchored_present_group_exhausts_deadline_without_receipt",
        "present",
        |log| {
            assert!(
                log.contains("result=0 errno=0"),
                "fixture did not report group present: {log}"
            );
        },
    )
}

fn run_fixture(
    test_name: &str,
    mode: &str,
    verify_log: impl FnOnce(&str),
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = TestDirectory::new()?;
    let library = directory.path().join("observe.dylib");
    let log_path = directory.path().join("interpose.log");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/group_signal_zero_interpose.c");
    let compile = Command::new("/usr/bin/cc")
        .args(["-dynamiclib", "-o"])
        .arg(&library)
        .arg(source)
        .output()?;
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let exact_name = format!("permission_retry_darwin::{test_name}");
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["--exact", &exact_name, "--nocapture"])
        .env("VELNOR_GROUP_PROBE_CHILD", "1")
        .env("VELNOR_GROUP_PROBE_MODE", mode)
        .env("VELNOR_GROUP_PROBE_LOG", &log_path)
        .env("VELNOR_GROUP_PROBE_DIRECTORY", directory.path())
        .env("DYLD_INSERT_LIBRARIES", &library);
    let output = run_bounded_child(command, Duration::from_secs(10))?;
    assert!(
        output.status.success(),
        "child failed: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_to_string(log_path)?;
    assert!(
        log.contains("interposed pgid=-"),
        "fixture did not activate: {log}"
    );
    verify_log(&log);
    Ok(())
}

fn in_fixture_child() -> bool {
    std::env::var_os("VELNOR_GROUP_PROBE_CHILD").is_some()
}

fn run_bounded_child(mut command: Command, timeout: Duration) -> io::Result<Output> {
    command.process_group(0);
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output(),
            Ok(None) => {}
            Err(error) => {
                let cleanup = terminate_and_reap(&mut child);
                return Err(match cleanup {
                    Ok(()) => io::Error::other(format!("fixture child wait failed: {error}")),
                    Err(cleanup_error) => io::Error::other(format!(
                        "fixture child wait failed: {error}; cleanup failed: {cleanup_error}"
                    )),
                });
            }
        }
        if Instant::now() >= deadline {
            let cleanup = terminate_and_reap(&mut child);
            let output = child.wait_with_output()?;
            cleanup?;
            return Err(io::Error::other(format!(
                "fixture child exceeded {timeout:?}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_and_reap(child: &mut std::process::Child) -> io::Result<()> {
    let group_kill = if let Ok(raw_pid) = i32::try_from(child.id())
        && let Some(pid) = Pid::from_raw(raw_pid)
    {
        kill_process_group(pid, Signal::KILL)
    } else {
        Err(rustix::io::Errno::INVAL)
    };
    let child_kill = if group_kill.is_err() {
        child.kill()
    } else {
        Ok(())
    };
    let wait = child.wait().map(|_| ());
    wait?;
    child_kill?;
    match group_kill {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}

struct ReleaseOnDrop(PathBuf);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        drop(fs::write(&self.0, b"release"));
    }
}

fn deadline(duration: Duration) -> Result<CheckDeadline, velnor_actions_mise::MiseError> {
    CheckDeadline::after(duration)
}

fn shell(script: &str) -> Result<IsolatedCommand, velnor_actions_mise::MiseError> {
    IsolatedCommand::repo_task(
        "/bin/sh",
        vec![OsString::from("-c"), OsString::from(script)],
        &[],
    )
}

fn shell_with_args(
    script: &str,
    arguments: &[&Path],
) -> Result<IsolatedCommand, velnor_actions_mise::MiseError> {
    let mut args = vec![
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("probe"),
    ];
    args.extend(arguments.iter().map(|path| path.as_os_str().to_owned()));
    IsolatedCommand::repo_task("/bin/sh", args, &[])
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-mise-permission-retry-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}
