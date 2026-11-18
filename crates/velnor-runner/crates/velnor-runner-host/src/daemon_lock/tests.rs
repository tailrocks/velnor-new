// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{DaemonLock, EngineLineageGuard};
use crate::error::HostError;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn unlinking_the_lock_file_releases_the_claim_before_a_second_process_takes_it() -> TestResult<()> {
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

const INSTANCE: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn a_replaced_lineage_lock_file_fails_verification_closed() -> TestResult<()> {
    let root = tempfile::tempdir()?;
    let journal = root.path().join("journal.db");
    std::fs::write(&journal, b"")?;
    let engine = "engine-1";
    let first = EngineLineageGuard::acquire_at(engine, root.path())?;
    first.verify_lineage(&journal, INSTANCE, 1)?;
    let lock = std::fs::read_dir(root.path())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "lock"))
        .ok_or("lineage lock file missing")?;
    assert!(matches!(
        EngineLineageGuard::acquire_at(engine, root.path()),
        Err(HostError::Lock)
    ));
    first.verify_lineage(&journal, INSTANCE, 1)?;
    std::fs::remove_file(&lock)?;
    let second = EngineLineageGuard::acquire_at(engine, root.path())?;
    assert!(matches!(
        first.verify_lineage(&journal, INSTANCE, 1),
        Err(HostError::Lock)
    ));
    assert!(matches!(first.advance_revision(2), Err(HostError::Lock)));
    second.verify_lineage(&journal, INSTANCE, 1)?;
    Ok(())
}
