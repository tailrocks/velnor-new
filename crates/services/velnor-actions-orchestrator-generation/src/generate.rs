//! Staged `.github` replacement and outside-repo preview writes.
//! In-place commits exchange atomically on Linux/macOS, else fall back
//! to a guarded two-rename commit preserving old output on failure.

use std::path::{Path, PathBuf};

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_contract_config::ExecutionMode;
use velnor_actions_contract_workflow::expand_workflow;
use velnor_actions_workflow_renderer::render_tree_with_extra_and_preserved_template;
use velnor_actions_workflow_tree::guard::{self, SafeTreePath};
use velnor_actions_workflow_tree::marker::rehead_actionlint_marker;
use velnor_actions_workflow_tree::rendered::RenderedTree;

use crate::finalized::owned_preparation;
use crate::prepare::GenerationPreparation;
use crate::provenance::{ProfileProvenance, profile_provenance};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_pins::pins::resolve_mise_setup;
use velnor_actions_orchestrator_staged_validation::validate::validate_staged;
use velnor_actions_orchestrator_staged_validation::write::write_tree;

/// Filesystem guards: snapshots, destination validation, and ownership.
pub(crate) mod guards;

/// Re-exported snapshot: the `generate::ToolSnapshot` path is stable API.
pub use velnor_actions_orchestrator_discovery::tool_snapshot::ToolSnapshot;

use guards::{GenerateOwnership, prepare_preview_dir, same_filesystem};

pub(crate) mod preserved_template;

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
    let tofu_roots = velnor_actions_orchestrator_discovery::select_tofu::tofu_selected_roots(
        &prep.discovery.statuses,
    );
    let tofu_locks = velnor_actions_tofu_core::TofuLockSnapshot::capture(&prep.root, &tofu_roots);
    let rendered = render_staged_tree_with_snapshot(prep, dispatch)?;
    let validated_by = validate_staged(&rendered.tree)?;
    tools.verify(&prep.root)?;
    tofu_locks
        .verify(&prep.root)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let warnings = match &opts.output_dir {
        None => replace_in_place(
            &prep.root,
            &rendered.tree,
            rendered.preserved_template.as_deref(),
        )?,
        Some(dir) => {
            write_preview(prep, dir, &rendered.tree)?;
            Vec::new()
        }
    };
    Ok(GenerateReport {
        files_written: rendered.tree.paths(),
        recommendations: prep.discovery.recommendations.clone(),
        validated_by,
        profiles: profile_provenance(prep),
        warnings,
    })
}

/// Fail closed on transient-only profile evidence before touching `.github`.
fn fail_on_blocking_findings(prep: &GenerationPreparation) -> Result<(), OrchestratorError> {
    let blockers = velnor_actions_orchestrator_discovery::evidence::blocking_findings(
        &prep.discovery.workspaces,
    );
    if !blockers.is_empty() {
        return Err(OrchestratorError::Profile {
            problem: format!(
                "{}: {}",
                velnor_actions_rust_core::TRANSIENT_EVIDENCE_CODE,
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
    Ok(render_staged_tree_with_snapshot(prep, dispatch)?.tree)
}

/// Render the tree and retain the exact repository-owned input snapshot used.
fn render_staged_tree_with_snapshot(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedGeneration, OrchestratorError> {
    let owned = owned_preparation(prep)?;
    let rendered = render_all(&owned, dispatch)?;
    check_tree_paths(&rendered.tree)?;
    Ok(rendered)
}

/// In-memory output and the optional template bytes from which it was rendered.
struct RenderedGeneration {
    tree: RenderedTree,
    preserved_template: Option<String>,
}

/// Render base files plus release extras into the marker-checked tree, in memory only.
fn render_all(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedGeneration, OrchestratorError> {
    let version = env!("CARGO_PKG_VERSION");
    let mise = resolve_mise_setup(&prep.config, &prep.runner_label)?;
    let execution_mode = if prep.config.schema == 2 {
        dispatch.or(prep
            .config
            .execution
            .as_ref()
            .and_then(|execution| execution.mode))
    } else {
        None
    };
    let ir = expand_workflow(&prep.workflow.ir, &prep.config, dispatch).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let rendered =
        velnor_actions_workflow_render_strict::render_workflow_ir_strict_shared_with_mode(
            &ir,
            prep.config.workflow.policy,
            prep.workflow.support.as_ref(),
            &prep.workflow.context,
            &mise,
            execution_mode,
        )?;
    let workflow = rendered.yaml;
    let actionlint = render_actionlint_yaml(&prep.workflow.actionlint)?;
    let actionlint = rehead_actionlint_marker(&actionlint.yaml, version)?;
    let mut extra = crate::release_emit::release_files(prep, &mise)?;
    extra.extend(crate::freshness_emit::freshness_files(prep)?);
    extra.extend(crate::routing::extra_files(&prep.config, version)?);
    extra.extend(rendered.shared);
    let preserved_template = preserved_template::read(&prep.root)?;
    let tree = render_tree_with_extra_and_preserved_template(
        &workflow,
        &actionlint,
        &extra,
        preserved_template.as_deref(),
        version,
    )?;
    Ok(RenderedGeneration {
        tree,
        preserved_template,
    })
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
/// The preserved-input snapshot is rechecked after staging, immediately
/// before replacement. The lock coordinates generators, not unrelated writers.
/// Success may carry old-tree-removal warnings.
fn replace_in_place(
    root: &Path,
    tree: &RenderedTree,
    preserved_template: Option<&str>,
) -> Result<Vec<String>, OrchestratorError> {
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
    preserved_template::ensure_unchanged(root, preserved_template)?;
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

#[cfg(test)]
mod tests;
