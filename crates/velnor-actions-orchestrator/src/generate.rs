//! Atomic `.github` replacement and outside-repo preview writes.

use std::path::{Path, PathBuf};

use velnor_actions_actionlint::render_actionlint_yaml;
use velnor_actions_workflow_renderer::guard::{self, SafeTreePath};
use velnor_actions_workflow_renderer::render::{RenderedTree, render_tree, render_workflow_ir};

use crate::OrchestratorError;
use crate::prepare::GenerationPreparation;
use crate::validate::validate_staged;

/// Options for [`generate`].
#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    /// Preview root; `None` replaces the in-repo `.github` tree atomically.
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
}

/// Render in memory, then replace `.github` atomically or write a preview.
///
/// In-place replacement stages under the root and swaps directories, so a
/// failed render, validation, or swap leaves the previous tree unchanged.
/// Preview mode writes `PATH/.github` only when `PATH` is absent or empty,
/// outside the repository, and not an ancestor of it.
///
/// # Errors
///
/// Returns render, actionlint, preview, unsafe-path, or IO errors.
pub fn generate(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
) -> Result<GenerateReport, OrchestratorError> {
    let tree = render_all(prep)?;
    check_tree_paths(&tree)?;
    let validated_by = validate_staged(&tree)?;
    match &opts.output_dir {
        None => replace_in_place(prep, &tree)?,
        Some(dir) => write_preview(prep, dir, &tree)?,
    }
    Ok(GenerateReport {
        files_written: tree.files.iter().map(|file| file.path.clone()).collect(),
        recommendations: prep.discovery.recommendations.clone(),
        validated_by,
    })
}

/// Render both files plus the marker-checked two-file tree, in memory only.
fn render_all(prep: &GenerationPreparation) -> Result<RenderedTree, OrchestratorError> {
    let version = env!("CARGO_PKG_VERSION");
    let workflow = render_workflow_ir(
        &prep.workflow.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
    )?;
    let actionlint = render_actionlint_yaml(&prep.workflow.actionlint)?;
    let actionlint = rehead_marker(&actionlint.yaml, version)?;
    Ok(render_tree(&workflow, &actionlint, version)?)
}

/// Replace the actionlint header line with the renderer marker.
///
/// The actionlint adapter emits its own header comment while the renderer
/// requires its exact marker on every tree file; the composition boundary
/// normalizes the first line only and passes the body through untouched.
fn rehead_marker(actionlint_yaml: &str, version: &str) -> Result<String, OrchestratorError> {
    let Some((_, body)) = actionlint_yaml.split_once('\n') else {
        return Err(OrchestratorError::Render {
            problem: "actionlint_without_header".to_owned(),
        });
    };
    let marker = velnor_actions_workflow_renderer::marker::marker_for_version(version)?;
    Ok(format!("{marker}\n{body}"))
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

/// Atomically replace `<root>/.github` via staging plus directory swap.
fn replace_in_place(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
) -> Result<(), OrchestratorError> {
    let root = &prep.root;
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
    swap_directories(root, &target, &staged)
}

/// Swap the staged tree into place, restoring the old tree on failure.
fn swap_directories(root: &Path, target: &Path, staged: &Path) -> Result<(), OrchestratorError> {
    let backup = backup_path(root, target);
    let had_target = target.exists();
    if had_target {
        std::fs::rename(target, &backup)
            .map_err(|err| OrchestratorError::io(target.display().to_string(), err.to_string()))?;
    }
    if let Err(err) = std::fs::rename(staged, target) {
        if had_target && std::fs::rename(&backup, target).is_err() {
            // Best-effort restore failed; the swap error below is reported.
        }
        return Err(OrchestratorError::io(
            target.display().to_string(),
            err.to_string(),
        ));
    }
    if had_target {
        std::fs::remove_dir_all(&backup)
            .map_err(|err| OrchestratorError::io(backup.display().to_string(), err.to_string()))?;
    }
    Ok(())
}

/// A sibling backup path that does not exist yet.
fn backup_path(root: &Path, target: &Path) -> PathBuf {
    let pid = std::process::id();
    let base = format!(".github.velnor-backup.{pid}");
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

/// Create or validate the preview root, refusing unsafe destinations.
fn prepare_preview_dir(root: &Path, dest: &Path) -> Result<PathBuf, OrchestratorError> {
    let label = dest.display().to_string();
    if let Ok(meta) = std::fs::symlink_metadata(dest) {
        if meta.is_symlink() {
            return Err(OrchestratorError::PreviewRefused {
                path: label,
                reason: "symlink_refused".to_owned(),
            });
        }
        if !meta.is_dir() {
            return Err(OrchestratorError::PreviewRefused {
                path: label,
                reason: "not_a_directory".to_owned(),
            });
        }
        let mut entries = std::fs::read_dir(dest)
            .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
        if entries.next().is_some() {
            return Err(OrchestratorError::PreviewRefused {
                path: label,
                reason: "non_empty".to_owned(),
            });
        }
    } else {
        std::fs::create_dir_all(dest)
            .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    }
    let canonical = dest
        .canonicalize()
        .map_err(|err| OrchestratorError::io(label.clone(), err.to_string()))?;
    if canonical == *root || canonical.starts_with(root) {
        return Err(OrchestratorError::PreviewRefused {
            path: label,
            reason: "inside_repository".to_owned(),
        });
    }
    if root.starts_with(&canonical) {
        return Err(OrchestratorError::PreviewRefused {
            path: label,
            reason: "ancestor_of_repository".to_owned(),
        });
    }
    Ok(canonical)
}
/// Write every rendered file under `github_dir`.
pub(crate) fn write_tree(github_dir: &Path, tree: &RenderedTree) -> Result<(), OrchestratorError> {
    for file in &tree.files {
        let rel = file.path.strip_prefix(".github/").unwrap_or(&file.path);
        let dest = github_dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                OrchestratorError::io(parent.display().to_string(), err.to_string())
            })?;
        }
        std::fs::write(&dest, &file.bytes)
            .map_err(|err| OrchestratorError::io(dest.display().to_string(), err.to_string()))?;
    }
    Ok(())
}
