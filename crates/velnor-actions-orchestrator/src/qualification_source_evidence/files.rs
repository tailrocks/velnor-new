//! Read bounded canonical files from downloaded report artifacts.

use std::env;
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use velnor_actions_contract::{canonical_json_bytes, parse_strict_json};

use crate::OrchestratorError;
use crate::internal::internal;

use super::RECEIPT_FILENAME;

pub(super) fn artifact_root(
    reports: &Path,
    artifact_id: &str,
    required_file: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let artifact = reports.join(artifact_id);
    let candidates = [artifact.clone(), artifact.join(artifact_id)];
    let existing = candidates
        .into_iter()
        .filter(|root| artifact_file_exists(root, required_file))
        .collect::<Vec<_>>();
    if existing.len() != 1 {
        return Err(internal("qualification_receipt_artifact_layout_invalid"));
    }
    let mut roots = existing.into_iter();
    let root = roots
        .next()
        .ok_or_else(|| internal("qualification_receipt_artifact_layout_invalid"))?;
    if has_symlink_component(reports, &root)? {
        return Err(internal("qualification_receipt_artifact_symlink"));
    }
    Ok(root)
}

fn artifact_file_exists(root: &Path, relative: &Path) -> bool {
    let path = root.join(relative);
    !has_symlink_component(root, &path).unwrap_or(true)
        && std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn has_symlink_component(base: &Path, path: &Path) -> Result<bool, OrchestratorError> {
    if !path.starts_with(base) {
        return Err(internal("qualification_receipt_path_escape"));
    }
    let mut current = base.to_path_buf();
    if crate::retrieve_reports::staged_reads::path_is_symlink(base) {
        return Ok(true);
    }
    for component in path
        .strip_prefix(base)
        .map_err(|_| internal("qualification_receipt_path_escape"))?
        .components()
    {
        current.push(component.as_os_str());
        if crate::retrieve_reports::staged_reads::path_is_symlink(&current) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn read_canonical<T: DeserializeOwned + Serialize>(
    path: &Path,
    limit: u64,
) -> Result<T, OrchestratorError> {
    let bytes = read_checked_bytes(path, limit)?;
    parse_canonical(&bytes)
}

pub(super) fn parse_canonical<T: DeserializeOwned + Serialize>(
    bytes: &[u8],
) -> Result<T, OrchestratorError> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| internal("qualification_receipt_encoding"))?;
    let value = parse_strict_json(text).map_err(|_| internal("qualification_receipt_json"))?;
    let decoded: T =
        serde_json::from_value(value).map_err(|_| internal("qualification_receipt_shape"))?;
    if canonical_json_bytes(&decoded).map_err(crate::internal::internal_contract)? != bytes {
        return Err(internal("qualification_receipt_noncanonical"));
    }
    Ok(decoded)
}

pub(super) fn read_checked_bytes(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    let parent = path
        .parent()
        .ok_or_else(|| internal("qualification_receipt_path_invalid"))?;
    if has_symlink_component(Path::new("/"), parent)? {
        return Err(internal("qualification_receipt_artifact_symlink"));
    }
    crate::retrieve_reports::staged_reads::read_staged_bytes(path, limit)
        .map_err(|kind| internal(&format!("qualification_receipt_file_{kind}")))
}

pub(super) fn reports_directory(runner_temp: &Path, run_key: &str) -> PathBuf {
    runner_temp.join("velnor").join(run_key).join("reports")
}

pub(super) fn required_path(name: &str) -> Result<PathBuf, OrchestratorError> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal(&format!("qualification_receipt_missing_{name}")))
}

pub(super) fn receipt_path(runner_temp: &Path, run_key: &str) -> PathBuf {
    runner_temp
        .join("velnor")
        .join(run_key)
        .join(RECEIPT_FILENAME)
}
