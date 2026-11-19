// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use velnor_runner_host::{DaemonLock, HostError};

type TestResult<T> = Result<T, Box<dyn Error>>;

const REFUSAL: &str = "daemon already running";
const BOUND: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(50);

fn daemon(state: &Path, stderr: Stdio) -> TestResult<Child> {
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("--state")
        .arg(state)
        .arg("daemon")
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()?)
}

fn wait_until_first_holds_the_lock(first: &mut Child, lock_path: &Path) -> TestResult<()> {
    let deadline = Instant::now() + BOUND;
    loop {
        if let Some(status) = first.try_wait()? {
            return Err(std::io::Error::other(format!(
                "first daemon exited before taking the lock: {status}"
            ))
            .into());
        }
        match DaemonLock::try_acquire(lock_path) {
            Err(HostError::Lock) => return Ok(()),
            Ok(probe) => drop(probe),
            Err(error) => {
                return Err(std::io::Error::other(format!(
                    "unexpected lock probe error: {error:?}"
                ))
                .into());
            }
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::other("first daemon did not take the lock").into());
        }
        sleep(POLL);
    }
}

fn reap_and_read(second: &mut Child) -> TestResult<String> {
    second.kill()?;
    second.wait()?;
    let mut text = String::new();
    if let Some(mut pipe) = second.stderr.take() {
        pipe.read_to_string(&mut text)?;
    }
    Ok(text)
}

#[test]
fn second_daemon_process_is_refused_after_the_lock_file_is_unlinked() -> TestResult<()> {
    let state = tempfile::Builder::new()
        .prefix("velnor-daemon-lock-process")
        .tempdir()?;
    let lock_path = state.path().join("daemon.lock");
    let mut first = daemon(state.path(), Stdio::null())?;
    wait_until_first_holds_the_lock(&mut first, &lock_path)?;
    std::fs::remove_file(&lock_path)?;
    let mut second = daemon(state.path(), Stdio::piped())?;
    let deadline = Instant::now() + BOUND;
    let mut exit = None;
    while exit.is_none() {
        if let Some(status) = second.try_wait()? {
            exit = Some(status);
        }
        if Instant::now() >= deadline {
            break;
        }
        sleep(POLL);
    }
    let second_stderr = reap_and_read(&mut second)?;
    if let Some(status) = exit {
        assert_eq!(status.code(), Some(1));
        assert!(
            second_stderr.contains(REFUSAL),
            "second daemon stderr lacks the refusal: {second_stderr}"
        );
        assert!(
            !lock_path.exists(),
            "refused second daemon recreated {lock_path:?}"
        );
        assert!(
            first.try_wait()?.is_none(),
            "first daemon exited after the second was refused"
        );
    } else {
        assert!(
            lock_path.exists(),
            "serving second daemon left no replacement {lock_path:?}"
        );
        assert!(
            !second_stderr.contains(REFUSAL),
            "serving second daemon printed the refusal: {second_stderr}"
        );
        return Err(std::io::Error::other(
            "second daemon kept serving after the lock file was unlinked",
        )
        .into());
    }
    first.kill()?;
    first.wait()?;
    Ok(())
}
