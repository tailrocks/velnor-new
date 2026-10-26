use std::path::{Path, PathBuf};

#[path = "rust_install_audit.rs"]
mod rust_install_audit;

#[path = "mise_task_suites_emit.rs"]
mod mise_task_suites_emit;

use velnor_actions_contract::ExecutionMode;
use velnor_actions_workflow_renderer::guard;
use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
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

#[path = "generate_stage_root.rs"]
mod stage_root;

pub use guards::ToolSnapshot;

use guards::GenerateOwnership;
use leaf_links::check_no_symlink_or_emitted_leaf;

#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    pub output_dir: Option<PathBuf>,
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
    let validated_by = validate_staged(&tree)?;
    tools.verify(&prep.root)?;
    tofu_locks
        .verify(&prep.root)
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let mut warnings = match &opts.output_dir {
        None => replace_in_place(prep, &tree)?,
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

#[path = "cache_cleanup_emit.rs"]
mod cache_cleanup_emit;

#[path = "generate_leaf_links.rs"]
mod leaf_links;

fn replace_in_place(
    prep: &GenerationPreparation,
    tree: &RenderedTree,
) -> Result<Vec<String>, OrchestratorError> {
    let root = &prep.root;
    let _ownership = GenerateOwnership::acquire(root)?;
    let target = root.join(".github");
    reject_symlink(&target)?;
    let staging = stage_root::StageRoot::open(root)?;
    let target_identity = staging.validate_target(&target)?;
    staging.validate_target_directories(&target, target_identity)?;
    reject_symlink(&target.join("workflows"))?;
    for rel in check_tree_paths(tree)? {
        check_no_symlink_or_emitted_leaf(root, &rel)?;
    }
    staging.clear_spare_before_staging()?;
    let staged = match preserve::stage_in_place(
        &target,
        staging.spare(),
        tree,
        &prep.config.workflow.preserve_workflows,
    ) {
        Ok(path) => path,
        Err(error) => return Err(staging.abort_before_commit(error)),
    };
    if let Err(error) = staging.validate_staged(&target, target_identity, &staged) {
        return Err(staging.abort_before_commit(error));
    }
    publish_in_place(&staging, &target, target_identity, &staged)
}

fn publish_in_place(
    staging: &stage_root::StageRoot,
    target: &Path,
    target_identity: Option<stage_root::FsIdentity>,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    match target_identity {
        Some(retired) => publish_existing_in_place(staging, target, retired, staged),
        None => publish_first_in_place(staging, target, staged),
    }
}

fn publish_existing_in_place(
    staging: &stage_root::StageRoot,
    target: &Path,
    retired: stage_root::FsIdentity,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    if let Err(error) = staging.validate_target_directories(target, Some(retired)) {
        return Err(staging.abort_before_commit(error));
    }
    if let Err(error) = exchange_directories(staged, target) {
        return Err(staging.abort_before_commit(OrchestratorError::io(
            target.display().to_string(),
            error.to_string(),
        )));
    }
    match staging.clean_retired_root(retired) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => Ok(vec![format!("backup_cleanup_failed:{error}")]),
    }
}

fn publish_first_in_place(
    staging: &stage_root::StageRoot,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    match std::fs::rename(staged, target) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => Err(staging.abort_before_commit(OrchestratorError::io(
            target.display().to_string(),
            error.to_string(),
        ))),
    }
}

fn swap_directories(
    _root: &Path,
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
    swap_existing_directory(target, staged, exchange_directories)
}

fn swap_existing_directory(
    target: &Path,
    staged: &Path,
    exchange: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<Vec<String>, OrchestratorError> {
    let label = target.display().to_string();
    exchange(staged, target).map_err(|err| OrchestratorError::io(label, err.to_string()))?;
    let old = staged.display().to_string();
    match std::fs::remove_dir_all(staged) {
        Ok(()) => Ok(Vec::new()),
        Err(cleanup) => Ok(vec![format!("backup_cleanup_failed:{old}:{cleanup}")]),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exchange_directories(staged: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, staged, CWD, target, RenameFlags::EXCHANGE).map_err(std::io::Error::from)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exchange_directories(_staged: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic directory exchange is unsupported on this platform",
    ))
}

#[path = "generate_write.rs"]
pub(crate) mod write;

pub(crate) use write::write_tree;
