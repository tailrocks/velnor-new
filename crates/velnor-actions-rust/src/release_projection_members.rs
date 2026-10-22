use super::{
    BTreeSet, ContractError, MAX_GLOB_PATTERNS, MAX_GLOB_STEPS, MAX_MANIFESTS, MAX_REGULAR_PATHS,
    glob_matches, has_glob_metacharacters, invalid, join_path, parent, parse_manifest, relative_to,
    string_array, validate_pattern, validate_repo_path,
};

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
