//! Transactional preview output publication.

use std::path::Path;

use velnor_actions_workflow_renderer::render::RenderedTree;

use crate::OrchestratorError;
use crate::generate::guards::prepare_preview_dir;
use crate::generate::{check_tree_paths, leaf_links::is_symlink, preserve, swap_directories};
use crate::prepare::GenerationPreparation;

/// Write `PATH/.github`, removing the reservation when staging fails.
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

/// Build preview output beside `.github`, then publish it by directory swap.
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
