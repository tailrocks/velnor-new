//! Safe reading of the one repository-owned `.github` input.

use std::fs::{self, Metadata};
use std::path::Path;

use velnor_actions_contract_release::formats::{
    MAX_PRESERVED_GITHUB_INPUT_BYTES, PULL_REQUEST_TEMPLATE_PATH,
};
use velnor_actions_orchestrator_core::{OrchestratorError, safe_read::stream_repo_file};

/// Read the optional template as bounded UTF-8 without following links.
/// Refuse replacement if the repository-owned template drifted after rendering.
///
/// Called under the generator ownership lock immediately before the staged
/// `.github` directory is swapped into place. The lock coordinates generator
/// processes; unrelated writers can still race after this check.
pub(crate) fn ensure_unchanged(
    root: &Path,
    expected: Option<&str>,
) -> Result<(), OrchestratorError> {
    if read(root)?.as_deref() == expected {
        return Ok(());
    }
    Err(OrchestratorError::Contract {
        problem: format!(
            "preserved_template_changed_during_generation:{PULL_REQUEST_TEMPLATE_PATH}"
        ),
    })
}

pub(crate) fn read(root: &Path) -> Result<Option<String>, OrchestratorError> {
    let directory = root.join(".github");
    let Some(directory_metadata) = path_metadata(&directory)? else {
        return Ok(None);
    };
    validate_path_type(&directory, &directory_metadata, true)?;

    let path = root.join(PULL_REQUEST_TEMPLATE_PATH);
    let Some(file_metadata) = path_metadata(&path)? else {
        return Ok(None);
    };
    validate_path_type(&path, &file_metadata, false)?;
    if file_metadata.len() > MAX_PRESERVED_GITHUB_INPUT_BYTES {
        return Err(OrchestratorError::io(
            path.display().to_string(),
            "oversize",
        ));
    }

    let mut bytes = Vec::new();
    stream_repo_file(
        root,
        PULL_REQUEST_TEMPLATE_PATH,
        MAX_PRESERVED_GITHUB_INPUT_BYTES,
        |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))
}

fn path_metadata(path: &Path) -> Result<Option<Metadata>, OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(OrchestratorError::io(
            path.display().to_string(),
            error.to_string(),
        )),
    }
}

fn validate_path_type(
    path: &Path,
    metadata: &Metadata,
    directory: bool,
) -> Result<(), OrchestratorError> {
    let reason = if metadata.file_type().is_symlink() {
        Some("symlink_refused")
    } else if directory && !metadata.is_dir() {
        Some("not_a_directory")
    } else if !directory && !metadata.is_file() {
        Some("not_a_file")
    } else {
        None
    };
    match reason {
        Some(reason) => Err(OrchestratorError::UnsafePath {
            path: path.display().to_string(),
            reason: reason.to_owned(),
        }),
        None => Ok(()),
    }
}
