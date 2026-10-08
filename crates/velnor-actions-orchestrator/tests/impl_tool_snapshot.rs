//! Tool snapshot coverage: `.mise-version`/`.mise.lock` are snapshotted,
//! and missing files stay distinct from unreadable ones.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::ToolSnapshot;

use crate::impl_common::{TestResult, err_of};

#[test]
fn snapshot_tracks_mise_version_and_lock_files() -> TestResult {
    let dir = TempDir::new()?;
    let root = dir.path();
    fs::write(root.join(".mise-version"), "2026.10.4\n")?;
    fs::write(root.join(".mise.lock"), "lock-bytes")?;
    let snap = ToolSnapshot::capture(root);
    assert!(snap.verify(root).is_ok(), "unchanged verifies");
    fs::write(root.join(".mise-version"), "2026.10.3\n")?;
    let err = err_of(snap.verify(root), "version drift")?;
    assert!(
        err.to_string().contains("tool_files_changed:.mise-version"),
        "got {err}"
    );
    fs::write(root.join(".mise-version"), "2026.10.4\n")?;
    fs::write(root.join(".mise.lock"), "rotated-bytes")?;
    let err = err_of(snap.verify(root), "lock drift")?;
    assert!(
        err.to_string().contains("tool_files_changed:.mise.lock"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn snapshot_missing_files_stay_missing() -> TestResult {
    let dir = TempDir::new()?;
    let root = dir.path();
    let snap = ToolSnapshot::capture(root);
    assert!(
        snap.verify(root).is_ok(),
        "absent tool files are missing, never unreadable"
    );
    Ok(())
}

#[test]
fn snapshot_unreadable_version_file_fails_closed() -> TestResult {
    let dir = TempDir::new()?;
    let root = dir.path();
    fs::write(root.join(".mise-version"), "2026.10.4\n")?;
    let snap = ToolSnapshot::capture(root);
    assert!(snap.verify(root).is_ok(), "readable verifies");
    fs::remove_file(root.join(".mise-version"))?;
    fs::create_dir(root.join(".mise-version"))?;
    let err = err_of(snap.verify(root), "unreadable version file")?;
    assert!(
        err.to_string()
            .contains("tool_files_unreadable:.mise-version"),
        "got {err}"
    );
    Ok(())
}
