// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_runner_host::{DaemonLock, HostError};

type TestResult<T> = Result<T, Box<dyn Error>>;

#[test]
fn a_second_daemon_is_refused_while_the_first_holds_an_unlinked_lock_file() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("daemon.lock");
    let first = DaemonLock::try_acquire_daemon(state.path())?;
    std::fs::remove_file(&path)?;
    let second = DaemonLock::try_acquire_daemon(state.path());
    assert!(matches!(second, Err(HostError::Lock)));
    assert!(!path.exists());
    assert!(first.is_held());
    Ok(())
}

#[test]
fn a_daemon_can_start_again_once_the_first_releases_the_lock() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("daemon.lock");
    let first = DaemonLock::try_acquire_daemon(state.path())?;
    std::fs::remove_file(&path)?;
    drop(first);
    let second = DaemonLock::try_acquire_daemon(state.path())?;
    assert!(second.is_held());
    Ok(())
}
