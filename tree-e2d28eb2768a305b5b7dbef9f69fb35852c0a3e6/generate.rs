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

/// Re-exported snapshot: the `generate::ToolSnapshot` path is stable API.
pub use guards::ToolSnapshot;

use guards::{GenerateOwnership, prepare_preview_dir, same_filesystem};

/// Repository-owned content carried into the replacement transaction.
#[path = "generate_preserve.rs"]
mod preserve;

#[path = "generate_output_commit.rs"]
mod output_commit;

use output_commit::swap_directories;

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
        Some(dir) => {
            write_preview(prep, dir, &tree)?;
            Vec::new()
        }
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
    extra.extend(crate::routing::extra_files(&prep.config, version)?);
    extra.extend(rendered.shared);
    extra.extend(crate::foundation_qualification::files(prep)?);
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
    let permissions = preserve::root_permissions(&target)?;
    // Existing roots stage as siblings: read-only directory moves must stay
    // within one parent on macOS. Restore the final mode before publication.
    let staged = if permissions.is_some() {
        staging.path().to_path_buf()
    } else {
        staging.path().join(".github")
    };
    let directories = preserve::copy_repository_content(&target, &staged)?;
    preserve::check_generated_collisions(&staged, tree)?;
    write_tree(&staged, tree)?;
    preserve::restore_directory_permissions(directories)?;
    preserve::restore_root_permissions(&staged, permissions)?;
    if !same_filesystem(staging.path(), root)? {
        return Err(OrchestratorError::Contract {
            problem: "cross_filesystem_staging".to_owned(),
        });
    }
    swap_directories(root, &target, &staged)
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
    let github = canonical.join(".github");
    let permissions = preserve::root_permissions(&prep.root.join(".github"))?;
    let directories = preserve::copy_repository_content(&prep.root.join(".github"), &github)?;
    preserve::check_generated_collisions(&github, tree)?;
    write_tree(&github, tree)?;
    preserve::restore_directory_permissions(directories)?;
    preserve::restore_root_permissions(&github, permissions)
}

/// Staged tree writing: regular files and symbolic links.
#[path = "generate_write.rs"]
pub(crate) mod write;

pub(crate) use write::write_tree;
