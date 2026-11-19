// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::DaemonLock;
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

thread_local! {
    static FAIL_DIRECTORY_SYNC: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub(super) fn take_directory_sync_failure() -> bool {
    FAIL_DIRECTORY_SYNC.with(|count| {
        let remaining = count.get();
        count.set(remaining.saturating_sub(1));
        remaining > 0
    })
}

const INSTANCE: &str = "0123456789abcdef0123456789abcdef";

fn guard_at_revision_one(
    root: &std::path::Path,
    journal: &std::path::Path,
) -> TestResult<super::EngineLineageGuard> {
    let guard = super::EngineLineageGuard::acquire_at("engine-a", root)?;
    guard.verify_lineage(journal, INSTANCE, 1)?;
    Ok(guard)
}

#[test]
fn advance_revision_survives_a_directory_sync_failure_after_the_rename() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let journal = state.path().join("journal.sqlite");
    let guard = guard_at_revision_one(&state.path().join("anchors"), &journal)?;

    FAIL_DIRECTORY_SYNC.with(|count| count.set(1));
    assert!(matches!(guard.advance_revision(2), Err(HostError::Journal)));
    // The rename already published revision 2; memory is still on 1.
    assert_eq!(
        super::read_anchor(&guard.inner.anchor_path)?.map(|a| a.revision),
        Some(2)
    );

    guard.advance_revision(2)?;
    guard.advance_revision(3)?;
    assert_eq!(
        super::read_anchor(&guard.inner.anchor_path)?.map(|a| a.revision),
        Some(3)
    );
    Ok(())
}

#[test]
fn advance_revision_keeps_failing_while_the_directory_sync_keeps_failing() -> TestResult<()> {
    let state = tempfile::tempdir()?;
    let journal = state.path().join("journal.sqlite");
    let guard = guard_at_revision_one(&state.path().join("anchors"), &journal)?;

    FAIL_DIRECTORY_SYNC.with(|count| count.set(2));
    assert!(matches!(guard.advance_revision(2), Err(HostError::Journal)));
    assert!(matches!(guard.advance_revision(2), Err(HostError::Journal)));
    guard.advance_revision(2)?;
    Ok(())
}

#[test]
fn advance_revision_rejects_an_anchor_that_is_not_the_requested_revision_or_identity()
-> TestResult<()> {
    let state = tempfile::tempdir()?;
    let journal = state.path().join("journal.sqlite");
    let guard = guard_at_revision_one(&state.path().join("anchors"), &journal)?;
    let current = super::read_anchor(&guard.inner.anchor_path)?.ok_or("anchor")?;

    let mut ahead = current.clone();
    ahead.revision = 3;
    super::write_anchor(&guard.inner.anchor_path, &ahead)?;
    assert!(matches!(guard.advance_revision(2), Err(HostError::Journal)));

    let mut other = current.clone();
    other.revision = 2;
    other.instance_id = "f".repeat(32);
    super::write_anchor(&guard.inner.anchor_path, &other)?;
    assert!(matches!(guard.advance_revision(2), Err(HostError::Journal)));

    let mut moved = current;
    moved.revision = 2;
    moved.journal_path = "/elsewhere/journal.sqlite".to_owned();
    super::write_anchor(&guard.inner.anchor_path, &moved)?;
    assert!(matches!(guard.advance_revision(2), Err(HostError::Journal)));
    Ok(())
}
