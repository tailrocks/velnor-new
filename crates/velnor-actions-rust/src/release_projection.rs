//! Bounded, source-authenticated Cargo manifest projection for binary releases.
//!
//! This is intentionally not a replacement for Cargo metadata. It projects
//! only the selected workspace and configured binary products needed by the
//! release contract, using exact-source TOML blobs and a caller-authenticated
//! inventory of regular paths. It never runs Cargo or reads the filesystem.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ContractError,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProduct},
};

use crate::metadata::{PackageRecord, TargetRecord, VelnorV1TaskOwner, WorkspaceRecord};

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_MANIFESTS: usize = 4096;
const MAX_TOTAL_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_REGULAR_PATHS: usize = 250_000;
const MAX_GLOB_STEPS: usize = 16_000_000;
const MAX_GLOB_PATTERNS: usize = 128;
const MAX_REPO_PATH_BYTES: usize = 1024;
const MAX_GLOB_PATTERN_BYTES: usize = 1024;

struct ParsedReleaseManifests<'a> {
    values: BTreeMap<&'a str, toml::Value>,
    names: BTreeMap<String, String>,
}

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

/// Project selected release products from exact-source manifest bytes.
///
/// The `manifests` key set must exactly match the workspace closure returned
/// by [`release_workspace_manifest_paths`]. Each key must identify a regular
/// Cargo.toml in the authenticated source inventory. Only configured release
/// products are represented in the returned record; this is not a complete
/// Cargo dependency graph.
///
/// # Errors
/// Rejects incomplete or extra manifest input, ambiguous package names,
/// invalid workspace inheritance, unavailable binaries, and unsafe paths.
pub fn project_release_workspace(
    config: &RustBinaryReleaseConfig,
    manifests: &BTreeMap<String, Vec<u8>>,
    regular_paths: &BTreeSet<String>,
) -> Result<WorkspaceRecord, ContractError> {
    config.validate(".velnor/config.toml")?;
    let root_bytes = manifests
        .get(&config.manifest_path)
        .ok_or_else(|| invalid("workspace_manifest_missing"))?;
    let expected =
        release_workspace_manifest_paths(&config.manifest_path, root_bytes, regular_paths)?;
    if manifests
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected.iter().map(String::as_str).collect()
    {
        return Err(invalid("manifest_closure_mismatch"));
    }
    let workspace_dir = parent(&config.manifest_path).unwrap_or("");
    let parsed = parse_member_manifests(config, manifests, regular_paths)?;
    validate_selected_products(config)?;
    let mut packages = config
        .products
        .iter()
        .map(|product| {
            project_selected_product(config, product, workspace_dir, &parsed, regular_paths)
        })
        .collect::<Result<Vec<_>, ContractError>>()?;
    packages.sort_by(|left, right| left.manifest.cmp(&right.manifest));
    let members = packages.iter().map(|package| package.id.clone()).collect();
    Ok(WorkspaceRecord {
        workspace_root: workspace_dir.to_owned(),
        members,
        packages,
        edges: Vec::new(),
        skipped_edges: Vec::new(),
        has_external_dependencies: true,
    })
}

fn parse_member_manifests<'a>(
    config: &RustBinaryReleaseConfig,
    manifests: &'a BTreeMap<String, Vec<u8>>,
    regular_paths: &BTreeSet<String>,
) -> Result<ParsedReleaseManifests<'a>, ContractError> {
    let mut parsed = BTreeMap::new();
    let mut names = BTreeMap::<String, String>::new();
    let mut total_bytes = 0usize;
    for (path, bytes) in manifests {
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("manifest_size"))?;
        if total_bytes > MAX_TOTAL_MANIFEST_BYTES {
            return Err(invalid("manifest_size"));
        }
        if !regular_paths.contains(path) {
            return Err(invalid("manifest_not_regular_source_file"));
        }
        let value = parse_manifest(path, bytes)?;
        let Some(package) = value.get("package").and_then(toml::Value::as_table) else {
            if path == &config.manifest_path && value.get("workspace").is_some() {
                parsed.insert(path.as_str(), value);
                continue;
            }
            return Err(invalid("workspace_member_package_missing"));
        };
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("package_name_missing"))?;
        if !valid_package_name(name) {
            return Err(invalid("package_name_invalid"));
        }
        if names.insert(name.to_owned(), path.clone()).is_some() {
            return Err(invalid("ambiguous_workspace_package_name"));
        }
        parsed.insert(path.as_str(), value);
    }
    Ok(ParsedReleaseManifests {
        values: parsed,
        names,
    })
}

fn validate_selected_products(config: &RustBinaryReleaseConfig) -> Result<(), ContractError> {
    let wanted = config
        .products
        .iter()
        .map(|product| product.package.as_str())
        .collect::<BTreeSet<_>>();
    if wanted.len() != config.products.len() {
        return Err(invalid("duplicate_product_package"));
    }
    if !wanted.contains(config.version_package.as_str()) {
        return Err(invalid("version_package_not_a_product"));
    }
    Ok(())
}

fn project_selected_product(
    config: &RustBinaryReleaseConfig,
    product: &RustBinaryReleaseProduct,
    workspace_dir: &str,
    parsed: &ParsedReleaseManifests<'_>,
    regular_paths: &BTreeSet<String>,
) -> Result<PackageRecord, ContractError> {
    let path = parsed
        .names
        .get(&product.package)
        .ok_or_else(|| invalid("missing_package"))?;
    let value = parsed
        .values
        .get(path.as_str())
        .ok_or_else(|| invalid("manifest_missing"))?;
    let binding = velnor_actions_contract::CargoVersionBinding {
        package: product.package.clone(),
        manifest_path: path.clone(),
        workspace_manifest_path: config.manifest_path.clone(),
    };
    let version = crate::release_versions::package_version(&binding, &parsed.values)?;
    let manifest_dir = parent(path).unwrap_or("");
    reject_nested_workspace_ancestor(manifest_dir, workspace_dir, regular_paths)?;
    ensure_member_ownership(value, &binding.manifest_path, workspace_dir)?;
    let workspace_manifest = parsed
        .values
        .get(config.manifest_path.as_str())
        .ok_or_else(|| invalid("workspace_manifest_missing"))?;
    let bins = product_bins(
        product,
        value,
        workspace_manifest,
        manifest_dir,
        regular_paths,
    )?;
    let package_table = value
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid("package_table_missing"))?;
    let features = value
        .get("features")
        .and_then(toml::Value::as_table)
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default();
    let has_build_script = build_script_exists(package_table, manifest_dir, regular_paths)?;
    Ok(PackageRecord {
        id: format!("release-path:{path}"),
        name: product.package.clone(),
        version,
        manifest: path.clone(),
        external: false,
        in_workspace: true,
        targets: bins,
        features,
        has_build_script,
        v1_task_owner: VelnorV1TaskOwner::Project,
    })
}

fn reject_nested_workspace_ancestor(
    manifest_dir: &str,
    workspace_dir: &str,
    regular_paths: &BTreeSet<String>,
) -> Result<(), ContractError> {
    let mut directory = parent(manifest_dir).unwrap_or("");
    while directory != workspace_dir && !directory.is_empty() {
        if regular_paths.contains(&join_path(directory, "Cargo.toml")) {
            return Err(invalid("nested_workspace_ownership_ambiguous"));
        }
        directory = parent(directory).unwrap_or("");
    }
    Ok(())
}

fn build_script_exists(
    package: &toml::Table,
    manifest_dir: &str,
    regular_paths: &BTreeSet<String>,
) -> Result<bool, ContractError> {
    match package.get("build") {
        Some(toml::Value::Boolean(false)) => Ok(false),
        Some(toml::Value::String(path)) => {
            regular_source_path(manifest_dir, path, regular_paths)?;
            Ok(true)
        }
        Some(toml::Value::Boolean(true)) | None => {
            Ok(regular_paths.contains(&join_path(manifest_dir, "build.rs")))
        }
        _ => Err(invalid("build_script_declaration")),
    }
}

fn effective_edition(
    package: &toml::Table,
    workspace_manifest: &toml::Value,
) -> Result<&'static str, ContractError> {
    let inherited = workspace_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("edition"))
        .and_then(toml::Value::as_str);
    let edition = match package.get("edition") {
        None => "2015",
        Some(toml::Value::String(edition)) => match edition.as_str() {
            "2015" => "2015",
            "2018" => "2018",
            "2021" => "2021",
            "2024" => "2024",
            _ => return Err(invalid("package_edition_unsupported")),
        },
        Some(toml::Value::Table(declaration))
            if declaration.len() == 1
                && declaration.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            match inherited {
                Some("2015") => "2015",
                Some("2018") => "2018",
                Some("2021") => "2021",
                Some("2024") => "2024",
                _ => return Err(invalid("workspace_edition_missing_or_unsupported")),
            }
        }
        _ => return Err(invalid("package_edition_declaration")),
    };
    Ok(edition)
}

fn product_bins(
    product: &RustBinaryReleaseProduct,
    manifest: &toml::Value,
    workspace_manifest: &toml::Value,
    manifest_dir: &str,
    regular_paths: &BTreeSet<String>,
) -> Result<Vec<TargetRecord>, ContractError> {
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid("package_table_missing"))?;
    let package_name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid("package_name_missing"))?;
    let explicit = manifest.get("bin").and_then(toml::Value::as_array);
    if manifest.get("bin").is_some() && explicit.is_none() {
        return Err(invalid("bin_declaration"));
    }
    for bin in explicit.into_iter().flatten() {
        if bin.get("name").and_then(toml::Value::as_str).is_none() {
            return Err(invalid("binary_name_missing"));
        }
    }
    let edition = effective_edition(package, workspace_manifest)?;
    let has_declared_bins = explicit.is_some_and(|bins| !bins.is_empty());
    let autobins = package
        .get("autobins")
        .and_then(toml::Value::as_bool)
        .unwrap_or(!(edition == "2015" && has_declared_bins));
    let mut result = Vec::new();
    for name in &product.bins {
        let matches = explicit
            .into_iter()
            .flatten()
            .filter(|bin| bin.get("name").and_then(toml::Value::as_str) == Some(name))
            .collect::<Vec<_>>();
        let (path, required_features) = match matches.as_slice() {
            [bin] => {
                let path = match bin.get("path") {
                    Some(toml::Value::String(path)) => {
                        regular_source_path(manifest_dir, path, regular_paths)?
                    }
                    Some(_) => return Err(invalid("binary_path_type")),
                    None => implicit_bin_path(name, package_name, manifest_dir, regular_paths)?,
                };
                (
                    path,
                    read_string_array(bin.get("required-features"), "required_features")?,
                )
            }
            [] if autobins => (
                implicit_bin_path(name, package_name, manifest_dir, regular_paths)?,
                Vec::new(),
            ),
            [] => return Err(invalid("binary_missing")),
            _ => return Err(invalid("ambiguous_binary_target")),
        };
        let _ = path;
        result.push(TargetRecord {
            kind: "bin".to_owned(),
            name: name.clone(),
            test: false,
            doctest: false,
            required_features,
        });
    }
    result.sort();
    if result.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(invalid("duplicate_binary_target"));
    }
    Ok(result)
}

fn implicit_bin_path(
    name: &str,
    package_name: &str,
    manifest_dir: &str,
    regular_paths: &BTreeSet<String>,
) -> Result<String, ContractError> {
    let mut candidates = vec![
        join_path(manifest_dir, &format!("src/bin/{name}.rs")),
        join_path(manifest_dir, &format!("src/bin/{name}/main.rs")),
    ];
    if name == package_name {
        candidates.push(join_path(manifest_dir, "src/main.rs"));
    }
    let present = candidates
        .into_iter()
        .filter(|path| regular_paths.contains(path))
        .collect::<Vec<_>>();
    match present.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(invalid("binary_source_missing")),
        _ => Err(invalid("ambiguous_binary_source")),
    }
}

fn ensure_member_ownership(
    manifest: &toml::Value,
    manifest_path: &str,
    workspace_dir: &str,
) -> Result<(), ContractError> {
    if let Some(workspace) = manifest
        .get("package")
        .and_then(|package| package.get("workspace"))
    {
        let relative = workspace
            .as_str()
            .ok_or_else(|| invalid("package_workspace_path"))?;
        if relative.starts_with('/') || relative.contains('\\') {
            return Err(invalid("package_workspace_path"));
        }
        let resolved =
            normalize_directory(&join_path(parent(manifest_path).unwrap_or(""), relative))?;
        if resolved != workspace_dir {
            return Err(invalid("package_workspace_mismatch"));
        }
    }
    if manifest.get("workspace").is_some()
        && manifest_path != join_path(workspace_dir, "Cargo.toml")
    {
        return Err(invalid("nested_workspace_manifest"));
    }
    Ok(())
}

fn regular_source_path(
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

fn parse_manifest(_path: &str, bytes: &[u8]) -> Result<toml::Value, ContractError> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("manifest_size"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("manifest_utf8"))?;
    toml::from_str(text).map_err(|_| invalid("manifest_toml"))
}

fn string_array(
    value: Option<&toml::Value>,
    reason: &'static str,
) -> Result<Vec<String>, ContractError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    read_string_array(Some(value), reason)
}

fn read_string_array(
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

fn validate_repo_path(path: &str) -> Result<(), ContractError> {
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

fn normalize_directory(path: &str) -> Result<String, ContractError> {
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

fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

fn relative_to<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    if base.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(base)?.strip_prefix('/')
    }
}

fn join_path(base: &str, path: &str) -> String {
    if base.is_empty() {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}

fn invalid(problem: &'static str) -> ContractError {
    ContractError::identity("release_projection", problem)
}
