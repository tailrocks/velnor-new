// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeSet, ContractError, MAX_GLOB_PATTERNS, MAX_GLOB_STEPS, MAX_MANIFESTS, MAX_REGULAR_PATHS,
    glob_matches, has_glob_metacharacters, invalid, join_path, parent, parse_manifest, relative_to,
    string_array, validate_pattern, validate_repo_path,
};

struct WorkspacePatterns<'a> {
    literals: BTreeSet<&'a str>,
    globs: Vec<&'a str>,
}

fn pattern_declaration_count(
    value: Option<&toml::Value>,
    reason: &'static str,
) -> Result<usize, ContractError> {
    value.map_or(Ok(0), |value| {
        value
            .as_array()
            .map(Vec::len)
            .ok_or_else(|| invalid(reason))
    })
}

fn partition_workspace_patterns<'a>(
    members: &'a [String],
    exclusions: &'a [String],
) -> Result<(WorkspacePatterns<'a>, WorkspacePatterns<'a>), ContractError> {
    for pattern in members.iter().chain(exclusions) {
        validate_pattern(pattern)?;
    }
    let glob_count = members
        .iter()
        .chain(exclusions)
        .filter(|pattern| has_glob_metacharacters(pattern))
        .count();
    if glob_count > MAX_GLOB_PATTERNS {
        return Err(invalid("workspace_inventory_limit"));
    }
    let partition = |patterns: &'a [String]| {
        let literals = patterns
            .iter()
            .filter(|pattern| !has_glob_metacharacters(pattern))
            .map(String::as_str)
            .collect();
        let globs = patterns
            .iter()
            .filter(|pattern| has_glob_metacharacters(pattern))
            .map(String::as_str)
            .collect();
        WorkspacePatterns { literals, globs }
    };
    Ok((partition(members), partition(exclusions)))
}

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
    let member_count = pattern_declaration_count(workspace.get("members"), "workspace_members")?;
    let exclusion_count = pattern_declaration_count(workspace.get("exclude"), "workspace_exclude")?;
    if exclusion_count > MAX_MANIFESTS
        || member_count > MAX_MANIFESTS - exclusion_count
        || regular_paths.len() > MAX_REGULAR_PATHS
    {
        return Err(invalid("workspace_inventory_limit"));
    }
    let members = string_array(workspace.get("members"), "workspace_members")?;
    let exclusion_patterns = string_array(workspace.get("exclude"), "workspace_exclude")?;
    if members.is_empty() && !package_at_root {
        return Err(invalid("workspace_members_empty"));
    }
    let (member_patterns, exclusion_patterns) =
        partition_workspace_patterns(&members, &exclusion_patterns)?;
    if !package_at_root && member_patterns.literals.contains(".") {
        return Err(invalid("virtual_workspace_root_member"));
    }
    for pattern in &member_patterns.literals {
        if *pattern == "." {
            continue;
        }
        let manifest = join_path(workspace_dir, &join_path(pattern, "Cargo.toml"));
        if !regular_paths.contains(&manifest) {
            return Err(invalid("workspace_member_manifest_missing"));
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
        let included = patterns_match(
            &member_patterns.literals,
            &member_patterns.globs,
            relative_dir,
            &mut glob_steps,
        )?;
        let matches_exclusion = patterns_match(
            &exclusion_patterns.literals,
            &exclusion_patterns.globs,
            relative_dir,
            &mut glob_steps,
        )?;
        if included && !matches_exclusion {
            selected.insert(path.clone());
        }
    }
    if selected.len() > MAX_MANIFESTS {
        return Err(invalid("workspace_manifest_limit"));
    }
    Ok(selected.into_iter().collect())
}

fn patterns_match(
    literals: &BTreeSet<&str>,
    globs: &[&str],
    path: &str,
    remaining_steps: &mut usize,
) -> Result<bool, ContractError> {
    if literals.contains(path) {
        return Ok(true);
    }
    for pattern in globs {
        if glob_matches(pattern, path, remaining_steps)? {
            return Ok(true);
        }
    }
    Ok(false)
}
