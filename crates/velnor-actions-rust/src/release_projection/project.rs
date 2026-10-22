use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ContractError,
    config::{RustBinaryReleaseConfig, RustBinaryReleaseProduct},
};

use crate::metadata::{PackageRecord, VelnorV1TaskOwner, WorkspaceRecord};

use super::product_targets::{product_bins, valid_package_name};
use super::source::{
    invalid, join_path, normalize_directory, parent, parse_manifest, regular_source_path,
};
use super::workspace::release_workspace_manifest_paths;

const MAX_TOTAL_MANIFEST_BYTES: usize = 16 * 1024 * 1024;

struct ParsedReleaseManifests<'a> {
    values: BTreeMap<&'a str, toml::Value>,
    names: BTreeMap<String, String>,
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
