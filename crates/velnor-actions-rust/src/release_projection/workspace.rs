use std::collections::BTreeSet;

use velnor_actions_contract::ContractError;

use super::source::{
    MAX_REPO_PATH_BYTES, invalid, join_path, parent, parse_manifest, relative_to, string_array,
    validate_repo_path,
};

const MAX_MANIFESTS: usize = 4096;
const MAX_REGULAR_PATHS: usize = 250_000;
const MAX_GLOB_STEPS: usize = 16_000_000;
const MAX_GLOB_PATTERNS: usize = 128;
const MAX_GLOB_PATTERN_BYTES: usize = 1024;

/// Return the exact member `Cargo.toml` paths implied by a root manifest.
///
/// `regular_paths` must be the complete, bounded regular-file inventory for
/// the authenticated source tree. Symlinks, gitlinks, missing paths and
/// truncated inventories must not be supplied as regular files.
///
/// # Errors
/// Rejects malformed workspace declarations, unsupported glob syntax,
/// path escapes, and invalid or oversized root TOML.
pub fn release_workspace_manifest_paths(
    workspace_manifest: &str,
    root_bytes: &[u8],
    regular_paths: &BTreeSet<String>,
) -> Result<Vec<String>, ContractError> {
    validate_repo_path(workspace_manifest)?;
    if !workspace_manifest.ends_with("Cargo.toml") {
        return Err(invalid("workspace_manifest_path"));
    }
    let root = parse_manifest(workspace_manifest, root_bytes)?;
    if !regular_paths.contains(workspace_manifest) {
        return Err(invalid("workspace_manifest_not_regular_source_file"));
    }
    let workspace = root.get("workspace").and_then(toml::Value::as_table);
    let package_at_root = root
        .get("package")
        .and_then(toml::Value::as_table)
        .is_some();
    let workspace_dir = parent(workspace_manifest).unwrap_or("");
    let mut selected = BTreeSet::new();
    selected.insert(workspace_manifest.to_owned());
    let Some(workspace) = workspace else {
        if !package_at_root {
            return Err(invalid("workspace_missing"));
        }
        return Ok(selected.into_iter().collect());
    };
    select_workspace_member_paths(
        workspace_manifest,
        workspace_dir,
        package_at_root,
        workspace,
        regular_paths,
    )
}

fn select_workspace_member_paths(
    workspace_manifest: &str,
    workspace_dir: &str,
    package_at_root: bool,
    workspace: &toml::Table,
    regular_paths: &BTreeSet<String>,
) -> Result<Vec<String>, ContractError> {
    let mut selected = BTreeSet::from([workspace_manifest.to_owned()]);
    let members = string_array(workspace.get("members"), "workspace_members")?;
    let exclusion_patterns = string_array(workspace.get("exclude"), "workspace_exclude")?;
    if members.len() + exclusion_patterns.len() > MAX_GLOB_PATTERNS
        || regular_paths.len() > MAX_REGULAR_PATHS
    {
        return Err(invalid("workspace_inventory_limit"));
    }
    if members.is_empty() && !package_at_root {
        return Err(invalid("workspace_members_empty"));
    }
    for pattern in members.iter().chain(exclusion_patterns.iter()) {
        validate_pattern(pattern)?;
    }
    if !package_at_root && members.iter().any(|pattern| pattern == ".") {
        return Err(invalid("virtual_workspace_root_member"));
    }
    for pattern in &members {
        if pattern == "." {
            continue;
        }
        if !has_glob_metacharacters(pattern) {
            let manifest = join_path(workspace_dir, &join_path(pattern, "Cargo.toml"));
            if !regular_paths.contains(&manifest) {
                return Err(invalid("workspace_member_manifest_missing"));
            }
        }
    }
    let mut glob_steps = MAX_GLOB_STEPS;
    let mut manifest_candidates = 0usize;
    for path in regular_paths {
        validate_repo_path(path)?;
        if !path.ends_with("/Cargo.toml") && path != "Cargo.toml" {
            continue;
        }
        if path == workspace_manifest {
            continue;
        }
        let Some(dir) = parent(path) else { continue };
        let Some(relative_dir) = relative_to(workspace_dir, dir) else {
            continue;
        };
        manifest_candidates += 1;
        if manifest_candidates > MAX_MANIFESTS {
            return Err(invalid("workspace_manifest_limit"));
        }
        let included = members.iter().try_fold(false, |matched, pattern| {
            if matched {
                Ok(true)
            } else {
                glob_matches(pattern, relative_dir, &mut glob_steps)
            }
        })?;
        let matches_exclusion = exclusion_patterns
            .iter()
            .try_fold(false, |matched, pattern| {
                if matched {
                    Ok(true)
                } else {
                    glob_matches(pattern, relative_dir, &mut glob_steps)
                }
            })?;
        if included && !matches_exclusion {
            selected.insert(path.clone());
        }
    }
    if selected.len() > MAX_MANIFESTS {
        return Err(invalid("workspace_manifest_limit"));
    }
    Ok(selected.into_iter().collect())
}

fn validate_pattern(pattern: &str) -> Result<(), ContractError> {
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

fn has_glob_metacharacters(pattern: &str) -> bool {
    pattern.bytes().any(|byte| matches!(byte, b'*' | b'?'))
}

fn glob_matches(
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

fn glob_match_at(
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
