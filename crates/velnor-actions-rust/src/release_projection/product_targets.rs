use std::collections::BTreeSet;

use velnor_actions_contract::{ContractError, config::RustBinaryReleaseProduct};

use crate::metadata::TargetRecord;

use super::source::{invalid, join_path, read_string_array, regular_source_path};

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

pub(super) fn product_bins(
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

pub(super) fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}
