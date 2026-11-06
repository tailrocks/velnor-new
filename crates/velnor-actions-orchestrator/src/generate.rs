// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::sync::Arc;

#[path = "rust_install_audit.rs"]
pub(crate) mod rust_install_audit;

#[path = "cache_authority_audit.rs"]
pub(crate) mod cache_authority_audit;

#[path = "mise_task_suites_emit.rs"]
mod mise_task_suites_emit;

use velnor_actions_contract::ExecutionMode;
use velnor_actions_workflow_renderer::guard;
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::generation_filesystem::{GenerationFilesystem, HostGenerationFilesystem};
use crate::prepare::GenerationPreparation;
use crate::provenance::{ProfileProvenance, profile_provenance};
use crate::validate::validate_staged;

#[path = "generate_guards.rs"]
pub(crate) mod guards;

#[path = "generate_preserve.rs"]
mod preserve;

#[path = "generate_runner_labels.rs"]
mod runner_labels;

#[path = "generate_render.rs"]
mod render;
use render::{check_tree_paths, render_all};

#[path = "generate_preflight.rs"]
mod preflight;

#[path = "generate_unmanaged.rs"]
pub(crate) mod unmanaged;

#[path = "generate_preview.rs"]
mod preview;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "generate_fd.rs"]
mod fd;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "generate_stage_root.rs"]
mod stage_root;

pub use guards::ToolSnapshot;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use guards::GenerateOwnership;

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub output_dir: Option<PathBuf>,
    pub filesystem: Arc<dyn GenerationFilesystem>,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            output_dir: None,
            filesystem: Arc::new(HostGenerationFilesystem::new()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateReport {
    pub files_written: Vec<String>,
    pub recommendations: Vec<String>,
    pub validated_by: Vec<String>,
    pub profiles: Vec<ProfileProvenance>,
    pub warnings: Vec<String>,
}

pub fn generate(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
) -> Result<GenerateReport, OrchestratorError> {
    generate_dispatched(prep, opts, None)
}

pub fn generate_dispatched(
    prep: &GenerationPreparation,
    opts: &GenerateOptions,
    dispatch: Option<ExecutionMode>,
) -> Result<GenerateReport, OrchestratorError> {
    preflight::fail_on_blocking_findings(prep)?;
    let notices = preflight::unmanaged_notices(prep)?;
    let tools = ToolSnapshot::capture(&prep.root);
    let tofu_roots = crate::select_tofu::tofu_selected_roots(&prep.discovery.statuses);
    let tofu_locks = velnor_actions_tofu::TofuLockSnapshot::capture(&prep.root, &tofu_roots);
    let tree = render_staged_tree_with(prep, dispatch)?;
    let validated_by = validate_staged(&tree, &prep.catalog)?;
    tools.verify(&prep.root)?;
    tofu_locks
        .verify(&prep.root)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let mut warnings = match &opts.output_dir {
        None => replace_in_place(prep, &tree, opts.filesystem.clone())?,
        Some(dir) => preview::write_preview(prep, dir, &tree)?,
    };
    warnings.extend(notices);
    Ok(GenerateReport {
        files_written: tree.paths(),
        recommendations: prep.discovery.recommendations.clone(),
        validated_by,
        profiles: profile_provenance(prep),
        warnings,
    })
}

pub fn render_staged_tree(prep: &GenerationPreparation) -> Result<RenderedTree, OrchestratorError> {
    render_staged_tree_with(prep, None)
}

pub fn render_staged_tree_with(
    prep: &GenerationPreparation,
    dispatch: Option<ExecutionMode>,
) -> Result<RenderedTree, OrchestratorError> {
    let tree = render_all(prep, dispatch)?;
    check_tree_paths(&tree)?;
    Ok(tree)
}

#[path = "cache_cleanup_emit.rs"]
mod cache_cleanup_emit;

#[path = "generate_leaf_links.rs"]
mod leaf_links;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn replace_in_place(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
    filesystem: Arc<dyn GenerationFilesystem>,
) -> Result<Vec<String>, OrchestratorError> {
    let root = stage_root::RootDirectory::open(&prep.root, filesystem)?;
    let _ownership = GenerateOwnership::acquire(&root)?;
    root.verify_location()?;
    let mut staging = stage_root::StageRoot::open(&root)?;
    let paths = check_tree_paths(tree)?;
    staging.verify_generated_paths(&paths)?;
    let source = match staging.target_directory() {
        Ok(source) => source,
        Err(error) => return Err(staging.abort_before_commit(error)),
    };
    let staged = match staging.create_staged_github() {
        Ok(staged) => staged,
        Err(error) => return Err(staging.abort_before_commit(error)),
    };
    if let Err(error) =
        preserve::stage_in_place(source.as_ref(), &staged, tree, staging.target_mode())
    {
        return Err(staging.abort_before_commit(error));
    }
    if let Err(error) = staging.validate_staged(&staged) {
        return Err(staging.abort_before_commit(error));
    }
    publish_in_place(&mut staging)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn replace_in_place(
    _prep: &GenerationPreparation,
    _tree: &RenderedTree,
    _filesystem: Arc<dyn GenerationFilesystem>,
) -> Result<Vec<String>, OrchestratorError> {
    Err(OrchestratorError::unsupported(
        "in_place_generation",
        "atomic_directory_exchange_requires_linux_or_macos",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn publish_in_place(
    staging: &mut stage_root::StageRoot<'_>,
) -> Result<Vec<String>, OrchestratorError> {
    if staging.target_identity().is_some() {
        if let Err(error) = staging.publish_existing() {
            return Err(staging.abort_before_commit(error));
        }
        match staging.cleanup_retired() {
            Ok(()) => Ok(Vec::new()),
            Err(error) => Ok(vec![format!("backup_cleanup_failed:{error}")]),
        }
    } else {
        if let Err(error) = staging.publish_first() {
            return Err(staging.abort_before_commit(error));
        }
        match staging.cleanup_after_first_publish() {
            Ok(()) => Ok(Vec::new()),
            Err(error) => Ok(vec![format!("backup_cleanup_failed:{error}")]),
        }
    }
}

#[path = "generate_write.rs"]
pub(crate) mod write;

pub(crate) use write::{write_tree, write_tree_at};
