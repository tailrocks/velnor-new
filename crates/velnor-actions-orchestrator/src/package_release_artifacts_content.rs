use std::{path::Path, time::Instant};

use serde_json::Value;
use velnor_actions_contract::parse_strict_json;

use super::super::{MAX_SBOM_BYTES, VERIFY_TIMEOUT, inventory};
use super::invalid;
use crate::OrchestratorError;

pub(super) fn inspect_sbom(
    path: &Path,
    archive_name: &str,
    archive_sha256: &str,
    started: Instant,
) -> Result<(), OrchestratorError> {
    let bytes = inventory::read_limited(path, MAX_SBOM_BYTES, started)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("preview_sbom_not_utf8"))?;
    let document = parse_strict_json(text).map_err(|_| invalid("preview_sbom_invalid_json"))?;
    let root = document
        .as_object()
        .ok_or_else(|| invalid("preview_sbom_root_invalid"))?;
    if root.get("bomFormat").and_then(Value::as_str) != Some("CycloneDX") {
        return Err(invalid("preview_sbom_format_invalid"));
    }
    let metadata = root
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("preview_sbom_metadata_invalid"))?;
    let component = metadata
        .get("component")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("preview_sbom_component_invalid"))?;
    if component.get("type").and_then(Value::as_str) != Some("file")
        || component.get("name").and_then(Value::as_str) != Some(archive_name)
    {
        return Err(invalid("preview_sbom_archive_name_mismatch"));
    }
    let hashes = component
        .get("hashes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("preview_sbom_hashes_missing"))?;
    let matches = hashes
        .iter()
        .filter(|hash| {
            hash.get("alg").and_then(Value::as_str) == Some("SHA-256")
                && hash.get("content").and_then(Value::as_str) == Some(archive_sha256)
        })
        .count();
    if matches != 1 || hashes.len() != 1 {
        return Err(invalid("preview_sbom_archive_digest_mismatch"));
    }
    Ok(())
}

pub(super) fn inspect_optional_identity_envelope(
    path: &Path,
    source_commit: &str,
    source_ref: &str,
    version: &str,
    started: Instant,
) -> Result<(), OrchestratorError> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Ok(());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if metadata.len() > MAX_SBOM_BYTES {
        return Ok(());
    }
    let bytes = inventory::read_limited(path, MAX_SBOM_BYTES, started)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("companion_json_not_utf8"))?;
    let Ok(value) = parse_strict_json(text) else {
        return Err(invalid("companion_json_invalid"));
    };
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    for (field, expected) in [
        ("source_commit", source_commit),
        ("source_ref", source_ref),
        ("version", version),
    ] {
        if let Some(value) = object.get(field)
            && value.as_str() != Some(expected)
        {
            return Err(invalid("companion_identity_mismatch"));
        }
    }
    Ok(())
}

pub(super) fn check_deadline(started: Instant) -> Result<(), OrchestratorError> {
    if velnor_actions_mise::CheckDeadline::check_scope().is_err()
        || started.elapsed() > VERIFY_TIMEOUT
    {
        return Err(invalid("package_output_verification_timeout"));
    }
    Ok(())
}
