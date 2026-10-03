use crate::OrchestratorError;
use crate::internal::internal;
use serde_json::json;
use std::fs;
use std::path::Path;

use super::inventory::owner;
use super::io;

pub(super) fn context_metadata_bytes(
    context: &str,
    endpoint: &str,
    metadata_dir: &Path,
) -> Result<Vec<u8>, OrchestratorError> {
    serde_json::to_vec(&json!({
        "Name": context,
        "Metadata": {},
        "Endpoints": {"docker": {"Host": endpoint, "SkipTLSVerify": false}},
        "TLSMaterial": {},
        "Storage": {"MetadataPath": metadata_dir, "TLSPath": ""}
    }))
    .map_err(|e| internal(&format!("container_context_metadata:{e}")))
}

pub(super) fn context_config_bytes(context: &str) -> Result<Vec<u8>, OrchestratorError> {
    serde_json::to_vec(&json!({"currentContext": context}))
        .map_err(|e| internal(&format!("container_context_config:{e}")))
}

pub(super) fn owned_directory(path: &Path, expected_owner: u32) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || owner(&metadata) != expected_owner
    {
        return Err(internal("container_context_directory"));
    }
    Ok(())
}

pub(super) fn owned_regular_file(
    path: &Path,
    expected_owner: u32,
) -> Result<bool, OrchestratorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()
            && !metadata.file_type().is_symlink()
            && owner(&metadata) == expected_owner),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io(path, error)),
    }
}

pub(super) fn ensure_children(path: &Path, expected: &[&str]) -> Result<(), OrchestratorError> {
    let mut actual = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| io(path, e))? {
        let entry = entry.map_err(|e| io(path, e))?;
        let file_name = entry.file_name();
        let name = file_name
            .to_str()
            .ok_or_else(|| internal("container_context_name"))?;
        actual.push(name.to_owned());
    }
    actual.sort_unstable();
    let mut expected = expected
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    expected.sort_unstable();
    if actual != expected {
        return Err(internal("container_context_extra_entry"));
    }
    Ok(())
}

pub(super) fn read_limited(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    crate::retrieve_reports::read_staged_bytes(path, 64 * 1024)
        .map_err(|_| internal("container_context_unreadable"))
}
