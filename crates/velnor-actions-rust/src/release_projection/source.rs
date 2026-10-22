use std::collections::BTreeSet;

use velnor_actions_contract::ContractError;

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_REPO_PATH_BYTES: usize = 1024;

pub(super) fn regular_source_path(
    base: &str,
    path: &str,
    regular_paths: &BTreeSet<String>,
) -> Result<String, ContractError> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return Err(invalid("source_path_escape"));
    }
    let resolved = normalize_path(&join_path(base, path))?;
    if !regular_paths.contains(&resolved) {
        return Err(invalid("binary_source_missing_or_nonregular"));
    }
    Ok(resolved)
}

pub(super) fn parse_manifest(_path: &str, bytes: &[u8]) -> Result<toml::Value, ContractError> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("manifest_size"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("manifest_utf8"))?;
    toml::from_str(text).map_err(|_| invalid("manifest_toml"))
}

pub(super) fn string_array(
    value: Option<&toml::Value>,
    reason: &'static str,
) -> Result<Vec<String>, ContractError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    read_string_array(Some(value), reason)
}

pub(super) fn read_string_array(
    value: Option<&toml::Value>,
    reason: &'static str,
) -> Result<Vec<String>, ContractError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| invalid(reason))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid(reason))
        })
        .collect()
}

pub(super) fn validate_repo_path(path: &str) -> Result<(), ContractError> {
    if path.is_empty()
        || path.len() > MAX_REPO_PATH_BYTES
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid("source_path_escape"));
    }
    Ok(())
}

fn normalize_path(path: &str) -> Result<String, ContractError> {
    let resolved = normalize_directory(path)?;
    if resolved.is_empty() {
        return Err(invalid("source_path_escape"));
    }
    Ok(resolved)
}

pub(super) fn normalize_directory(path: &str) -> Result<String, ContractError> {
    if path.starts_with('/') || path.contains('\\') {
        return Err(invalid("source_path_escape"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(invalid("source_path_escape"));
                }
            }
            value => parts.push(value),
        }
    }
    Ok(parts.join("/"))
}

pub(super) fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

pub(super) fn relative_to<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(base)?.strip_prefix('/')
    }
}

pub(super) fn join_path(base: &str, path: &str) -> String {
    if base.is_empty() {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

pub(super) fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("release_projection", problem)
}
