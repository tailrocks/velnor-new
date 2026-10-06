//! Staged `.github` replacement and outside-repo preview writes.
//! In-place commits exchange atomically on Linux/macOS, else fall back
//! to a guarded two-rename commit preserving old output on failure.

use std::path::{Path, PathBuf};

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_contract::{ExecutionMode, expand_workflow};
use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};
use velnor_actions_workflow_renderer::render::RenderedTree;
use velnor_actions_workflow_renderer::steps::rehead_actionlint_marker;
use velnor_actions_workflow_renderer::tree::render_tree_with_extra;

use crate::OrchestratorError;
use crate::finalized::owned_preparation;
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::provenance::{ProfileProvenance, profile_provenance};
use crate::validate::validate_staged;

/// Filesystem guards: snapshots, destination validation, and ownership.
#[path = "generate_guards.rs"]
pub(crate) mod guards;

/// Preserve repository-owned entries while replacing generated output.
#[path = "generate_preserve.rs"]
mod preserve;

/// Stage and publish fresh preview output transactionally.
#[path = "generate_preview.rs"]
mod preview;

/// Re-exported snapshot: the `generate::ToolSnapshot` path is stable API.
pub use guards::ToolSnapshot;

use guards::{GenerateOwnership, same_filesystem};

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
/// In-place replacement stages under the root and commits, leaving the
/// previous tree unchanged on failure; previews write `PATH/.github`.
///
/// # Errors
///
/// Returns profile, render, actionlint, preview, unsafe-path, or IO
/// errors; the profile gate runs before any write, leaving outputs untouched.
pub fn generate(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
) -> Result<GenerateReport, OrchestratorError> {
    generate_dispatched(prep, opts, None)
}

/// [`generate`] with an explicit dispatch mode overriding `execution.mode`.
///
/// # Errors
///
/// Same as [`generate`], plus routing errors.
pub fn generate_dispatched(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
    dispatch: Option<ExecutionMode>,
) -> Result<GenerateReport, OrchestratorError> {
    fail_on_blocking_findings(prep)?;
    let tools = ToolSnapshot::capture(&prep.root);
    let tofu_roots = crate::select_tofu::tofu_selected_roots(&prep.discovery.statuses);
    let tofu_locks = velnor_actions_tofu::TofuLockSnapshot::capture(&prep.root, &tofu_roots);
    let tree = render_staged_tree_with(prep, dispatch)?;
    let validated_by = validate_staged(&tree)?;
    tools.verify(&prep.root)?;
    tofu_locks
        .verify(&prep.root)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let warnings = match &opts.output_dir {
        None => replace_in_place(prep, &tree)?,
        Some(dir) => preview::write_preview(prep, dir, &tree)?,
    };
    Ok(GenerateReport {
        files_written: tree.paths(),
        recommendations: prep.discovery.recommendations.clone(),
        validated_by,
        profiles: profile_provenance(prep),
        warnings,
    })
}

/// Fail closed on transient-only profile evidence before touching `.github`.
fn fail_on_blocking_findings(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let blockers = crate::evidence::blocking_findings(&prep.discovery.workspaces);
    if !blockers.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "{}: {}",
                velnor_actions_rust::TRANSIENT_EVIDENCE_CODE,
                blockers.join("; ")
            ),
        });
    }
    if !prep.lock_audit_blocking.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "lock_audit_blocked: {}",
                prep.lock_audit_blocking.join("; ")
            ),
        });
    }
    Ok(())
}

/// Render the validated tree (base files plus release extras) in memory.
///
/// No writes, no validators.
///
/// # Errors
///
/// Returns lock, render, actionlint, or unsafe-path errors.
pub fn render_staged_tree(prep: &GenerationPreparation) -> Result<RenderedTree, OrchestratorError> {
    render_staged_tree_with(prep, None)
}

/// [`render_staged_tree`] with a dispatch-mode override.
///
/// # Errors
///
/// Same as [`render_staged_tree`].
pub fn render_staged_tree_with(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    let owned = owned_preparation(prep)?;
    let tree = render_all(&owned, dispatch)?;
    check_tree_paths(&tree)?;
    Ok(tree)
}

/// Render base files plus release extras into the marker-checked tree, in memory only.
fn render_all(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let ir = expand_workflow(&prep.workflow.ir, &prep.config, dispatch).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
        &mise,
    )?;
    let workflow = rendered.yaml;
    let actionlint = render_actionlint_yaml(&prep.workflow.actionlint)?;
    let actionlint = rehead_actionlint_marker(&actionlint.yaml, version)?;
    let mut extra = crate::release_emit::release_files(prep, &mise)?;
    extra.extend(crate::freshness_emit::freshness_files(prep)?);
    extra.extend(crate::owned_tool_publication::files(prep)?);
    extra.extend(crate::tofu_apply_emit::tofu_apply_files(prep)?);
    extra.extend(crate::routing::extra_files(&prep.config, version)?);
    extra.extend(rendered.shared);
    let tree = render_tree_with_extra(&workflow, &actionlint, &extra, version)?;
    Ok(tree)
}

/// Validate every rendered path lexically plus symlink-prefix probing.
fn check_tree_paths(tree: &RenderedTree) -> Result<Vec<SafeTreePath>, OrchestratorError> {
    let mut safe = Vec::with_capacity(tree.files.len());
    for file in &tree.files {
        safe.push(guard::validate_tree_path(&file.path)?);
    }
    for link in &tree.symlinks {
        guard::validate_tree_path(&link.path)?;
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

/// Stage under the root, then commit under the ownership lock.
///
/// Nothing before the commit touches old output; success may carry
/// old-tree-removal warnings.
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
    let staged = preserve::stage_in_place(&target, staging.path(), tree)?;
    if !same_filesystem(staging.path(), root)? {
        return Err(OrchestratorError::Contract {
            problem: "cross_filesystem_staging".to_owned(),
        });
    }
    swap_directories(root, &target, &staged)
}

/// Commit staged over target: fresh installs rename once (atomic), while
/// existing targets exchange atomically on Linux/macOS; elsewhere, and
/// where exchange is unsupported, the two-rename fallback keeps old output.
fn swap_directories(
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

/// Staged tree writing: regular files and symbolic links.
#[path = "generate_write.rs"]
pub(crate) mod write;

pub(crate) use write::write_tree;

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
