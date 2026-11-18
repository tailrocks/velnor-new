// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::DaemonLock;
use crate::error::HostError;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn unlinking_a_plain_lock_file_releases_the_claim_before_a_second_process_takes_it()
-> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("admission.lock");
    let first = DaemonLock::try_acquire(&path)?;
    assert!(first.is_held());
    std::fs::remove_file(&path)?;
    let second = DaemonLock::try_acquire(&path)?;
    assert!(second.is_held());
    assert!(!first.is_held());
    Ok(())
}

#[test]
fn a_second_acquire_on_the_linked_file_fails_and_the_first_stays_held() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("admission.lock");
    let first = DaemonLock::try_acquire(&path)?;
    assert!(matches!(
        DaemonLock::try_acquire(&path),
        Err(HostError::Lock)
    ));
    assert!(first.is_held());
    Ok(())
}

#[test]
fn a_daemon_lock_is_held_while_the_state_directory_flock_lives() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let path = state.path().join("daemon.lock");
    let first = DaemonLock::try_acquire_daemon(state.path())?;
    assert!(first.is_held());
    std::fs::remove_file(&path)?;
    assert!(first.is_held());
    assert!(matches!(
        DaemonLock::try_acquire_daemon(state.path()),
        Err(HostError::Lock)
    ));
    drop(first);
    let second = DaemonLock::try_acquire_daemon(state.path())?;
    assert!(second.is_held());
    Ok(())
}
