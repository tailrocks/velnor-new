use crate::OrchestratorError;
use crate::internal::internal;
use serde_json::json;
use std::fs;
use std::path::Path;
use velnor_actions_mise::CheckDeadline;

use super::inventory::owner;
use super::{RuntimeProjection, check_ancestors, io};

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

pub(super) fn ensure_children(
    path: &Path,
    expected: &[&str],
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    let mut actual = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| io(path, e))? {
        checkpoint(deadline)?;
        if actual.len() >= expected.len() {
            return Err(internal("container_context_extra_entry"));
        }
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
    checkpoint(deadline)?;
    Ok(())
}

pub(super) fn read_limited(
    path: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<Vec<u8>, OrchestratorError> {
    crate::retrieve_reports::staged_reads::read_staged_bytes_until(path, 64 * 1024, || {
        deadline_checkpoint(deadline)
    })
    .map_err(|_| internal("container_context_unreadable"))
}

pub(super) fn validate_owned_context(
    projection: &RuntimeProjection,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    checkpoint(deadline)?;
    let config = &projection.docker_config;
    let metadata = &projection.context_metadata;
    let config_file = config.join("config.json");
    let metadata_dir = config.join("contexts").join("meta");
    let hash_dir = metadata_dir.join(&projection.context_hash);
    if metadata.parent() != Some(hash_dir.as_path()) {
        return Err(internal("container_context_projection_path"));
    }
    for path in [
        &config_file,
        metadata,
        config,
        &config.join("contexts"),
        &metadata_dir,
        &hash_dir,
    ] {
        checkpoint(deadline)?;
        check_ancestors(path)?;
    }
    let expected_owner = projection.home_owner;
    for path in [config, &config.join("contexts"), &metadata_dir, &hash_dir] {
        checkpoint(deadline)?;
        owned_directory(path, expected_owner)?;
    }
    if !owned_regular_file(&config_file, expected_owner)?
        || !owned_regular_file(metadata, expected_owner)?
    {
        return Err(internal("container_context_projection_missing"));
    }
    ensure_children(config, &["config.json", "contexts"], deadline)?;
    ensure_children(&config.join("contexts"), &["meta"], deadline)?;
    ensure_children(&metadata_dir, &[&projection.context_hash], deadline)?;
    ensure_children(&hash_dir, &["meta.json"], deadline)?;
    let expected_config = context_config_bytes(&projection.context)?;
    let expected_metadata =
        context_metadata_bytes(&projection.context, &projection.endpoint, &hash_dir)?;
    if read_limited(&config_file, deadline)? != expected_config
        || read_limited(metadata, deadline)? != expected_metadata
    {
        return Err(internal("container_context_projection_changed"));
    }
    checkpoint(deadline)
}

fn checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| internal(&error.to_string()))?;
    }
    Ok(())
}

fn deadline_checkpoint(deadline: Option<CheckDeadline>) -> Result<(), &'static str> {
    deadline.map_or(Ok(()), |value| {
        value
            .remaining()
            .map(|_| ())
            .map_err(|_| "deadline_exhausted")
    })
}
