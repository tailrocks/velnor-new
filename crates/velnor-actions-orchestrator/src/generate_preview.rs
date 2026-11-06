// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::generate::guards::prepare_preview_dir;
use crate::generate::{check_tree_paths, leaf_links::is_symlink, preserve};
use crate::prepare::GenerationPreparation;

pub(super) fn write_preview(
    prep: &GenerationPreparation,
    dest: &Path,
    tree: &RenderedTree,
) -> Result<Vec<String>, OrchestratorError> {
    let canonical = prepare_preview_dir(&prep.root, dest)?;
    let result = write_staged_preview(prep, &canonical, tree);
    match result {
        Ok(warnings) => Ok(warnings),
        Err(failure) => match std::fs::remove_dir(canonical.join(".github")) {
            Ok(()) => Err(failure),
            Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => Err(failure),
            Err(cleanup) => Err(OrchestratorError::io(
                canonical.display().to_string(),
                format!("preview_failed:{failure}; reservation_cleanup_failed:{cleanup}"),
            )),
        },
    }
}

fn write_staged_preview(
    prep: &GenerationPreparation,
    canonical: &Path,
    tree: &RenderedTree,
) -> Result<Vec<String>, OrchestratorError> {
    for rel in check_tree_paths(tree)? {
        super::guard::check_no_symlink(canonical, &rel, is_symlink)?;
    }
    let staging = tempfile::tempdir_in(canonical)
        .map_err(|err| OrchestratorError::io(canonical.display().to_string(), err.to_string()))?;
    preserve::write_preview(&prep.root.join(".github"), staging.path(), tree)?;
    swap_directories(canonical, &canonical.join(".github"), staging.path())
}

fn swap_directories(
    _root: &Path,
    target: &Path,
    staged: &Path,
) -> Result<Vec<String>, OrchestratorError> {
    let label = target.display().to_string();
    if !target.exists() {
        return std::fs::rename(staged, target)
            .map(|()| Vec::new())
            .map_err(|error| OrchestratorError::io(label, error.to_string()));
    }
    exchange_directories(staged, target)
        .map_err(|error| OrchestratorError::io(label, error.to_string()))?;
    let old = staged.display().to_string();
    match std::fs::remove_dir_all(staged) {
        Ok(()) => Ok(Vec::new()),
        Err(error) => Ok(vec![format!("backup_cleanup_failed:{old}:{error}")]),
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
