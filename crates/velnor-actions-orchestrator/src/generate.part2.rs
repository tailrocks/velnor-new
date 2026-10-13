/// Temporary debug-build phase counter for the atomic replacement repro.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub static DEBUG_GENERATE_PHASE: AtomicUsize = AtomicUsize::new(0);

/// Filesystem guards: snapshots, destination validation, and ownership.
#[path = "generate_guards.rs"]
pub(crate) mod guards;

/// Preserve repository-owned entries while replacing generated output.
#[path = "generate_preserve.rs"]
mod preserve;

/// Stage and publish fresh preview output transactionally.
#[path = "generate_preview.rs"]
mod preview;

/// Own the private persistent spare used only for in-place generation.
#[path = "generate_stage_root.rs"]
mod stage_root;

#[path = "generate_output_commit.rs"]
mod output_commit;

/// Symlink-prefix guard with emitted-leaf replacement.
#[path = "generate_leaf_links.rs"]
mod leaf_links;

/// Re-exported snapshot: the `generate::ToolSnapshot` path is stable API.
pub use guards::ToolSnapshot;

use guards::GenerateOwnership;
use leaf_links::check_no_symlink_or_emitted_leaf;

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

/// Temporary owner used by the atomic-observer cleanup control.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct RetiredTreeOwners {
    directories: Vec<tempfile::TempDir>,
}

impl RetiredTreeOwners {
    /// Number of exchanged trees still owned by this control.
    #[doc(hidden)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.directories.len()
    }

    /// Whether this control owns no exchanged trees.
    #[doc(hidden)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.directories.is_empty()
    }
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
    generate_dispatched_with_retired_owners(prep, opts, dispatch, None)
}

/// Generate while retaining successfully exchanged trees for a bounded test.
#[doc(hidden)]
pub fn generate_with_retired_tree_owners(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
    retired: &mut RetiredTreeOwners,
) -> Result<GenerateReport, OrchestratorError> {
    generate_dispatched_with_retired_owners(prep, opts, None, Some(retired))
}

fn generate_dispatched_with_retired_owners(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
    dispatch: Option<ExecutionMode>,
    retired: Option<&mut RetiredTreeOwners>,
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
        None => replace_in_place(prep, &tree, retired)?,
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
    let mise_set = resolve_mise_setup_set(&prep.config, &prep.runner_label, &mise)?;
    let ir = expand_workflow(&prep.workflow.ir, &prep.config, dispatch).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let mut render_context = prep.workflow.context.clone();
    render_context.scale_set_selector = scale_set_selector_for_dispatch(&prep.config, dispatch)?;
    let mut actionlint_input = prep.workflow.actionlint.clone();
    actionlint_input.extra_runner_labels = render_context
        .scale_set_selector
        .as_ref()
        .map(|selector| selector.labels().to_vec())
        .unwrap_or_default();
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &render_context,
        &mise_set,
    )?;
    let workflow = rendered.yaml;
    let actionlint = render_actionlint_yaml(&actionlint_input)?;
    let actionlint = rehead_actionlint_marker(&actionlint.yaml, version)?;
    let mut extra = crate::release_emit::release_files(prep, &mise)?;
    extra.extend(crate::binary_release_emit::files(prep)?);
    extra.extend(crate::freshness_emit::freshness_files(prep)?);
    extra.extend(crate::owned_tool_publication::files(prep)?);
    extra.extend(crate::tofu_apply_emit::tofu_apply_files(prep)?);
    extra.extend(crate::routing::extra_files(&prep.config, version)?);
    extra.extend(rendered.shared);
    let mut tree = render_tree_with_extra(&workflow, &actionlint, &extra, version)?;
    crate::evidence::mbx_producer_passthrough::append_allowlisted(&prep.root, &mut tree)?;
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

include!("generate.part3.rs");
