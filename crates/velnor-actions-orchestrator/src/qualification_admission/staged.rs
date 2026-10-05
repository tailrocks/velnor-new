//! Runner-temp staging for the immutable qualification admission document.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::QualificationCacheAdmission;
use velnor_actions_contract::workflow::MAX_QUALIFICATION_RECEIPT_BYTES;

use crate::OrchestratorError;
use crate::internal::internal;
use crate::qualification_admission::QUALIFICATION_ADMISSION_FILENAME;

/// Require the deterministic admission output path to be absent before lookup.
pub(super) fn reject_stale(request_path: &Path) -> Result<(), OrchestratorError> {
    validate_request_parent(request_path)?;
    match fs::symlink_metadata(admission_path(request_path)?) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(internal("qualification_admission_path_unreadable")),
        Ok(_) => Err(internal("qualification_admission_already_exists")),
    }
}

/// Write one already bounded and parsed admission document exclusively.
pub(super) fn write_admission(request_path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let path = admission_path(request_path)?;
    validate_request_parent(request_path)?;
    crate::exclusive_write::write_exclusive(&path, bytes, "qualification_admission")
}

/// Read and parse the staged admission through the shared no-follow reader.
pub fn load_qualification_admission(
    request_path: &Path,
) -> Result<Option<QualificationCacheAdmission>, OrchestratorError> {
    let path = admission_path(request_path)?;
    validate_request_parent(request_path)?;
    if matches!(fs::symlink_metadata(&path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let max_bytes = u64::try_from(MAX_QUALIFICATION_RECEIPT_BYTES)
        .map_err(|_| internal("qualification_admission_size_invalid"))?;
    let text = crate::safe_read::read_event_file(&path, max_bytes)?;
    let admission = QualificationCacheAdmission::parse_bounded(text.as_bytes())
        .map_err(crate::internal::internal_contract)?;
    Ok(Some(admission))
}

fn admission_path(request_path: &Path) -> Result<PathBuf, OrchestratorError> {
    if request_path.file_name().and_then(|name| name.to_str()) != Some("plan-v1-request.json") {
        return Err(internal("qualification_request_filename_invalid"));
    }
    let parent = request_path
        .parent()
        .ok_or_else(|| internal("qualification_request_parent_missing"))?;
    Ok(parent.join(QUALIFICATION_ADMISSION_FILENAME))
}

fn validate_request_parent(request_path: &Path) -> Result<(), OrchestratorError> {
    let anchor = runner_temp()?;
    let parent = request_path
        .parent()
        .ok_or_else(|| internal("qualification_admission_parent_missing"))?;
    crate::exclusive_write::create_dir_no_symlink(&anchor, parent)?;
    require_under_anchor(&anchor, parent)
}

fn runner_temp() -> Result<PathBuf, OrchestratorError> {
    env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))
}

fn require_under_anchor(anchor: &Path, path: &Path) -> Result<(), OrchestratorError> {
    let canonical_anchor = anchor
        .canonicalize()
        .map_err(|_| internal("qualification_runner_temp_unavailable"))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| internal("qualification_admission_parent_unavailable"))?;
    if canonical_path.starts_with(canonical_anchor) {
        Ok(())
    } else {
        Err(internal("qualification_admission_path_outside_runner_temp"))
    }
}

#[cfg(test)]
#[path = "staged_tests.rs"]
mod tests;
