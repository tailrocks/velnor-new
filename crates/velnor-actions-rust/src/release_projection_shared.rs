use super::{ContractError, MAX_GLOB_PATTERN_BYTES, MAX_MANIFEST_BYTES, MAX_REPO_PATH_BYTES};

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

pub(super) fn validate_pattern(pattern: &str) -> Result<(), ContractError> {
    if pattern.is_empty()
        || pattern.len() > MAX_GLOB_PATTERN_BYTES
        || pattern.starts_with('/')
        || pattern.contains('\\')
        || pattern.split('/').any(|component| component == "..")
        || (pattern != "."
            && pattern
                .split('/')
                .any(|component| component.is_empty() || component == "."))
        || pattern.contains('{')
        || pattern.contains('}')
        || pattern.contains('[')
        || pattern.contains(']')
    {
        return Err(invalid("workspace_glob_unsupported"));
    }
    Ok(())
}

pub(super) fn has_glob_metacharacters(pattern: &str) -> bool {
    pattern.bytes().any(|byte| matches!(byte, b'*' | b'?'))
}

pub(super) fn glob_matches(
    pattern: &str,
    value: &str,
    remaining_steps: &mut usize,
) -> Result<bool, ContractError> {
    if value.len() > MAX_REPO_PATH_BYTES {
        return Err(invalid("source_path_too_long"));
    }
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let rows = pattern
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("workspace_glob_limit"))?;
    let columns = value
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("workspace_glob_limit"))?;
    let states = rows
        .checked_mul(columns)
        .ok_or_else(|| invalid("workspace_glob_limit"))?;
    if states > *remaining_steps {
        return Err(invalid("workspace_glob_work_limit"));
    }
    *remaining_steps -= states;
    let mut memo = vec![None; states];
    Ok(glob_match_at(pattern, value, 0, 0, columns, &mut memo))
}

pub(super) fn glob_match_at(
    pattern: &[u8],
    value: &[u8],
    p: usize,
    v: usize,
    columns: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let index = p * columns + v;
    if let Some(result) = memo[index] {
        return result;
    }
    let result = if p == pattern.len() {
        v == value.len()
    } else if pattern[p..].starts_with(b"**/") {
        ((v == 0 || value[v - 1] == b'/') && glob_match_at(pattern, value, p + 3, v, columns, memo))
            || (v < value.len()
                && if value[v] == b'/' {
                    glob_match_at(pattern, value, p + 3, v + 1, columns, memo)
                        || glob_match_at(pattern, value, p, v + 1, columns, memo)
                } else {
                    glob_match_at(pattern, value, p, v + 1, columns, memo)
                })
    } else if pattern[p..].starts_with(b"**") {
        glob_match_at(pattern, value, p + 2, v, columns, memo)
            || (v < value.len() && glob_match_at(pattern, value, p, v + 1, columns, memo))
    } else {
        match pattern[p] {
            b'*' => {
                glob_match_at(pattern, value, p + 1, v, columns, memo)
                    || (v < value.len()
                        && value[v] != b'/'
                        && glob_match_at(pattern, value, p, v + 1, columns, memo))
            }
            b'?' => {
                v < value.len()
                    && value[v] != b'/'
                    && glob_match_at(pattern, value, p + 1, v + 1, columns, memo)
            }
            byte => {
                v < value.len()
                    && byte == value[v]
                    && glob_match_at(pattern, value, p + 1, v + 1, columns, memo)
            }
        }
    };
    memo[index] = Some(result);
    result
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

pub(super) fn normalize_path(path: &str) -> Result<String, ContractError> {
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

pub(super) fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

pub(super) fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("release_projection", problem)
}
