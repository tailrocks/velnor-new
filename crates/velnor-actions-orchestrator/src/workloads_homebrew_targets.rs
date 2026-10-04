//! Filesystem admission before the pure native Homebrew target policy.

use crate::OrchestratorError;
use velnor_actions_contract::FileIndex;
use velnor_actions_native::homebrew::{audit::TapIdentity, targets::SourceTargets};

pub(super) fn admit(
    index: &FileIndex,
    tap: &TapIdentity,
) -> Result<SourceTargets, OrchestratorError> {
    let root = index
        .root()
        .canonicalize()
        .map_err(|_| failure("homebrew_source_missing"))?;
    for path in index.files() {
        if velnor_actions_native::homebrew::targets::is_target_source(path) {
            regular_source(&root, path)?;
        }
    }
    SourceTargets::from_inventory(index.files(), tap).map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })
}

fn regular_source(root: &std::path::Path, path: &str) -> Result<(), OrchestratorError> {
    let mut current = root.to_path_buf();
    for component in std::path::Path::new(path).components() {
        let std::path::Component::Normal(component) = component else {
            return Err(failure("homebrew_source_target_invalid"));
        };
        current.push(component);
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| failure("homebrew_source_target_missing"))?;
        if metadata.file_type().is_symlink() {
            return Err(failure("homebrew_source_target_symlink"));
        }
    }
    let metadata = std::fs::symlink_metadata(&current)
        .map_err(|_| failure("homebrew_source_target_missing"))?;
    if !metadata.is_file() {
        return Err(failure("homebrew_source_target_not_file"));
    }
    Ok(())
}

fn failure(reason: &str) -> OrchestratorError {
    crate::internal::internal(reason)
}
