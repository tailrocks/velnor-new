//! Commit staged `.github` output, preserving the old tree on failure.

use std::path::{Path, PathBuf};

use crate::OrchestratorError;

/// Commit staged over target: fresh installs rename once (atomic), while
/// existing targets exchange atomically on Linux/macOS; elsewhere, and
/// where exchange is unsupported, the two-rename fallback keeps old output.
pub(super) fn swap_directories(
    root: &Path,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    let label = target.display().to_string();
    if !target.exists() {
        return match std::fs::rename(staged, target) {
            Ok(()) => Ok(Vec::new()),
            Err(err) => Err(OrchestratorError::io(label, err.to_string())),
        };
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        use std::io::ErrorKind::{InvalidInput, PermissionDenied, Unsupported};
        match renameat_with(CWD, staged, CWD, target, RenameFlags::EXCHANGE) {
            Ok(()) => {
                let old = staged.display().to_string();
                return match std::fs::remove_dir_all(staged) {
                    Ok(()) => Ok(Vec::new()),
                    Err(cleanup) => Ok(vec![format!("backup_cleanup_failed:{old}:{cleanup}")]),
                };
            }
            Err(err) if matches!(err.kind(), Unsupported | InvalidInput | PermissionDenied) => {}
            Err(err) => return Err(OrchestratorError::io(label.clone(), err.to_string())),
        }
    }
    let backup = backup_path(root, target);
    std::fs::rename(target, &backup)
        .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    if let Err(commit) = std::fs::rename(staged, target) {
        let outcome = restore_backup(&backup, target);
        return Err(OrchestratorError::io(
            label,
            format!("commit_failed:{commit}; {outcome}"),
        ));
    }
    if let Err(cleanup) = std::fs::remove_dir_all(&backup) {
        let at = backup.display().to_string();
        return Ok(vec![format!("backup_cleanup_failed:{at}:{cleanup}")]);
    }
    Ok(Vec::new())
}

/// Restore `backup` to `target`, reporting `rolled_back` distinctly.
/// Rollback fails only under concurrent interference, never alone.
fn restore_backup(backup: &Path, target: &Path) -> String {
    match std::fs::rename(backup, target) {
        Ok(()) => "rolled_back".to_owned(),
        Err(rollback) => format!("rollback_failed:{rollback}"),
    }
}

/// A sibling backup path that does not exist yet.
fn backup_path(root: &Path, target: &Path) -> PathBuf {
    let base = format!(".github.velnor-backup.{}", std::process::id());
    let mut candidate = root.join(&base);
    let mut counter = 0u32;
    while candidate.exists() || candidate == *target {
        counter += 1;
        candidate = root.join(format!("{base}.{counter}"));
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> std::io::Result<PathBuf> {
        let dir = std::env::temp_dir().join(format!("velnor-gen-{test}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    #[test]
    fn commit_failure_preserves_old_tree_without_partial_write() {
        let root = scratch("commit_fail").expect("scratch");
        let target = root.join(".github");
        std::fs::create_dir_all(target.join("workflows")).expect("target");
        std::fs::write(target.join("workflows/old.yml"), "old: true\n").expect("old");
        let err =
            swap_directories(&root, &target, &root.join("missing-staged")).expect_err("fails");
        assert!(
            !target.join("actionlint.yaml").exists(),
            "no partial write: {err}"
        );
        assert_eq!(
            std::fs::read(target.join("workflows/old.yml")).expect("kept"),
            b"old: true\n"
        );
    }

    #[test]
    fn fresh_target_commit_failure_reports_without_rollback() {
        let root = scratch("fresh_commit_fail").expect("scratch");
        let target = root.join(".github");
        let err =
            swap_directories(&root, &target, &root.join("missing-staged")).expect_err("fails");
        assert!(!err.to_string().contains("rolled_back"), "{err}");
        assert!(!target.exists(), "nothing committed");
    }

    #[test]
    fn rollback_failure_preserved() {
        let root = scratch("rollback_fail").expect("scratch");
        let target = root.join(".github");
        std::fs::create_dir_all(&target).expect("target");
        std::fs::write(target.join("old.yml"), "old: true\n").expect("old");
        let outcome = restore_backup(&root.join("missing-backup"), &target);
        assert!(outcome.starts_with("rollback_failed:"), "{outcome}");
        assert_eq!(
            std::fs::read(target.join("old.yml")).expect("kept"),
            b"old: true\n"
        );
    }
}
