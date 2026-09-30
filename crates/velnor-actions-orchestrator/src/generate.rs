//! Staged `.github` replacement and outside-repo preview writes.
//!
//! In-place commits use two renames with a visibility gap between
//! them (a reader can transiently miss `.github`): staged, never atomic.

use std::io::Write;
use std::path::{Path, PathBuf};

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};
use velnor_actions_workflow_renderer::render::{
    RenderedTree, render_tree, render_workflow_ir_strict,
};
use velnor_actions_workflow_renderer::steps::rehead_actionlint_marker;

use crate::OrchestratorError;
use crate::attach::{attach_lock_acquire, attach_preseed};
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::provenance::{ProfileProvenance, profile_provenance};
use crate::validate::{validate_staged, verify_velnor_repository_files};

/// Filesystem guards: snapshots, destination validation, and ownership.
#[path = "generate_guards.rs"]
pub(crate) mod guards;

/// Re-exported snapshot: the `generate::ToolSnapshot` path is stable API.
pub use guards::ToolSnapshot;

use guards::{GenerateOwnership, prepare_preview_dir, same_filesystem};

/// Options for [`generate`].
#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    /// Preview root; `None` replaces the in-repo `.github` tree.
    pub output_dir: Option<PathBuf>,
}

/// Report for [`generate`].
#[derive(Debug, Clone)]
pub struct GenerateReport {
    /// Repository-relative files written, sorted.
    pub files_written: Vec<String>,
    /// Recommendations shared with `plan`.
    pub recommendations: Vec<String>,
    /// Pinned validators that accepted the staged tree, sorted.
    pub validated_by: Vec<String>,
    /// Per-workspace profile provenance.
    pub profiles: Vec<ProfileProvenance>,
    /// Cleanup warnings on a committed success (empty when clean).
    pub warnings: Vec<String>,
}

/// Render in memory, then replace `.github` or write a preview.
///
/// In-place replacement stages under the root and commits with two renames,
/// leaving the previous tree unchanged on failure. Preview writes
/// `PATH/.github` only for an absent/empty outside-repo non-ancestor root.
///
/// # Errors
///
/// Returns profile, render, actionlint, preview, unsafe-path, or IO
/// errors; the profile gate runs before any write, leaving outputs untouched.
pub fn generate(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
) -> Result<GenerateReport, OrchestratorError> {
    fail_on_blocking_findings(prep)?;
    let tools = ToolSnapshot::capture(&prep.root);
    let tree = render_staged_tree(prep)?;
    let validated_by = validate_staged(&tree)?;
    tools.verify(&prep.root)?;
    let warnings = match &opts.output_dir {
        None => replace_in_place(prep, &tree)?,
        Some(dir) => {
            write_preview(prep, dir, &tree)?;
            Vec::new()
        }
    };
    Ok(GenerateReport {
        files_written: tree.files.iter().map(|file| file.path.clone()).collect(),
        recommendations: prep.discovery.recommendations.clone(),
        validated_by,
        profiles: profile_provenance(prep),
        warnings,
    })
}

/// Fail closed on transient-only profile evidence before touching `.github`.
fn fail_on_blocking_findings(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let blockers = crate::evidence::blocking_findings(&prep.discovery.workspaces);
    if blockers.is_empty() {
        return Ok(());
    }
    Err(OrchestratorError::Profile {
        problem: format!(
            "{}: {}",
            velnor_actions_rust::TRANSIENT_EVIDENCE_CODE,
            blockers.join("; ")
        ),
    })
}

/// Render the validated two-file tree in memory; no writes, no validators.
///
/// # Errors
///
/// Returns lock, render, actionlint, or unsafe-path errors.
pub fn render_staged_tree(prep: &GenerationPreparation) -> Result<RenderedTree, OrchestratorError> {
    let mut owned = prep.clone();
    if prep.config.workflow.policy == WorkflowPolicy::VelnorRepositoryV1 {
        match verify_velnor_repository_files(&prep.root)? {
            Some(lock) => attach_lock_acquire(
                &mut owned.workflow.ir,
                &lock,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?,
            None => attach_preseed(
                &mut owned.workflow,
                &prep.runner_label,
                env!("CARGO_PKG_VERSION"),
            )?,
        }
    }
    let tree = render_all(&owned)?;
    check_tree_paths(&tree)?;
    Ok(tree)
}

/// Render both files plus the marker-checked two-file tree, in memory only.
fn render_all(prep: &GenerationPreparation) -> Result<RenderedTree, OrchestratorError> {
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let workflow = render_workflow_ir_strict(
        &prep.workflow.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
        &mise,
    )?;
    let actionlint = render_actionlint_yaml(&prep.workflow.actionlint)?;
    let actionlint = rehead_actionlint_marker(&actionlint.yaml, version)?;
    Ok(render_tree(&workflow, &actionlint, version)?)
}

/// Validate every rendered path lexically plus symlink-prefix probing.
fn check_tree_paths(tree: &RenderedTree) -> Result<Vec<SafeTreePath>, OrchestratorError> {
    let mut safe = Vec::with_capacity(tree.files.len());
    for file in &tree.files {
        safe.push(guard::validate_tree_path(&file.path)?);
    }
    Ok(safe)
}

/// Fail when a path exists as a symlink.
fn reject_symlink(path: &Path) -> Result<(), OrchestratorError> {
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.is_symlink()
    {
        return Err(OrchestratorError::UnsafePath {
            path: path.display().to_string(),
            reason: "symlink_refused".to_owned(),
        });
    }
    Ok(())
}

/// Probe for `check_no_symlink`: true when the prefix is a symlink.
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_symlink())
}

/// Stage under the root and commit with two renames (staged, not atomic).
///
/// Validation, staging, the same-filesystem check, and the commit run
/// under the ownership lock; nothing before the first rename touches
/// old output. Returns cleanup warnings when the commit lands but the
/// backup removal fails.
fn replace_in_place(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
) -> Result<Vec<String>, OrchestratorError> {
    let root = &prep.root;
    let _ownership = GenerateOwnership::acquire(root)?;
    let target = root.join(".github");
    reject_symlink(&target)?;
    reject_symlink(&target.join("workflows"))?;
    if target.exists() && !target.is_dir() {
        return Err(OrchestratorError::UnsafePath {
            path: target.display().to_string(),
            reason: "not_a_directory".to_owned(),
        });
    }
    for rel in check_tree_paths(tree)? {
        guard::check_no_symlink(root, &rel, is_symlink)?;
    }
    let staging = tempfile::tempdir_in(root)
        .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()))?;
    let staged = staging.path().join(".github");
    write_tree(&staged, tree)?;
    if !same_filesystem(staging.path(), root)? {
        return Err(OrchestratorError::Contract {
            problem: "cross_filesystem_staging".to_owned(),
        });
    }
    swap_directories(root, &target, &staged)
}

/// Commit with two renames, restoring old output on failure.
///
/// `target -> backup`, then `staged -> target` (staged, never atomic:
/// readers can transiently miss `.github`). Commit failure restores the
/// backup and reports `commit_failed:{commit}; {outcome}` distinctly.
fn swap_directories(
    root: &Path,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    let backup = backup_path(root, target);
    let had_target = target.exists();
    if had_target {
        std::fs::rename(target, &backup)
            .map_err(|err| OrchestratorError::io(target.display().to_string(), err.to_string()))?;
    }
    if let Err(commit) = std::fs::rename(staged, target) {
        if !had_target {
            return Err(OrchestratorError::io(
                target.display().to_string(),
                commit.to_string(),
            ));
        }
        let outcome = restore_backup(&backup, target);
        return Err(OrchestratorError::io(
            target.display().to_string(),
            format!("commit_failed:{commit}; {outcome}"),
        ));
    }
    if had_target && let Err(cleanup) = std::fs::remove_dir_all(&backup) {
        return Ok(vec![format!(
            "backup_cleanup_failed:{}:{cleanup}",
            backup.display()
        )]);
    }
    Ok(Vec::new())
}

/// Restore `backup` to `target`, reporting the rollback outcome distinctly.
///
/// Rollback only fails under concurrent interference between the two
/// renames (single-threaded it inverts a rename that just succeeded),
/// so this keeps the `rolled_back` / `rollback_failed:{err}` contract.
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

/// Write `PATH/.github` for a fresh outside-repo preview root.
fn write_preview(
    prep: &GenerationPreparation,
    dest: &Path,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    let canonical = prepare_preview_dir(&prep.root, dest)?;
    for rel in check_tree_paths(tree)? {
        guard::check_no_symlink(&canonical, &rel, is_symlink)?;
    }
    write_tree(&canonical.join(".github"), tree)
}

/// Write every rendered file under `github_dir`.
///
/// Parent chains are re-verified symlink-free immediately before use
/// and leaves are created exclusively: an existing leaf refuses as an
/// overwrite instead of truncating, and a swapped symlink refuses
/// instead of diverting the write.
pub(crate) fn write_tree(github_dir: &Path, tree: &RenderedTree) -> Result<(), OrchestratorError> {
    for file in &tree.files {
        let rel = file.path.strip_prefix(".github/").unwrap_or(&file.path);
        let dest = github_dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                OrchestratorError::io(parent.display().to_string(), err.to_string())
            })?;
            let mut current = Some(parent);
            while let Some(level) = current {
                if !level.starts_with(github_dir) {
                    break;
                }
                if std::fs::symlink_metadata(level).is_ok_and(|meta| meta.is_symlink()) {
                    return Err(OrchestratorError::UnsafePath {
                        path: level.display().to_string(),
                        reason: "symlink_refused".to_owned(),
                    });
                }
                current = if level == github_dir {
                    None
                } else {
                    level.parent()
                };
            }
        }
        if std::fs::symlink_metadata(&dest).is_ok_and(|meta| meta.is_symlink()) {
            return Err(OrchestratorError::UnsafePath {
                path: dest.display().to_string(),
                reason: "symlink_refused".to_owned(),
            });
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    OrchestratorError::OverwriteRefused {
                        path: dest.display().to_string(),
                    }
                } else {
                    OrchestratorError::io(dest.display().to_string(), err.to_string())
                }
            })?
            .write_all(file.bytes.as_bytes())
            .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))?;
    }
    Ok(())
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
    fn commit_failure_restores_backup_and_reports_both() {
        let root = scratch("rollback").expect("scratch");
        let target = root.join(".github");
        std::fs::create_dir_all(target.join("workflows")).expect("target");
        std::fs::write(target.join("workflows/old.yml"), "old: true\n").expect("old");
        let err =
            swap_directories(&root, &target, &root.join("missing-staged")).expect_err("fails");
        let text = err.to_string();
        assert!(
            text.contains("commit_failed") && text.contains("rolled_back"),
            "{text}"
        );
        assert_eq!(
            std::fs::read(target.join("workflows/old.yml")).expect("restored"),
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

    /// Rollback failure keeps the live target and names the rollback error.
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
