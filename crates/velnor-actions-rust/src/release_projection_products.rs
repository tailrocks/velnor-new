use super::{
    BTreeMap, BTreeSet, ContractError, MAX_TOTAL_MANIFEST_BYTES, PackageRecord,
    ParsedReleaseManifests, RustBinaryReleaseConfig, RustBinaryReleaseProduct, TargetRecord,
    VelnorV1TaskOwner, WorkspaceRecord, invalid, join_path, normalize_directory, normalize_path,
    parent, parse_manifest, read_string_array, release_workspace_manifest_paths,
    valid_package_name,
};

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
