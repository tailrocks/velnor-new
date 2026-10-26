// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::support::{invalid, io_error, path_exists};
use crate::OrchestratorError;

pub(super) fn verify_payload_directories(
    keg: &Path,
    expected: &BTreeSet<PathBuf>,
) -> Result<(), OrchestratorError> {
    for top in ["bin", "libexec"] {
        let root = keg.join(top);
        if !path_exists(&root)? {
            if expected.iter().any(|path| path.starts_with(top)) {
                return Err(invalid("installed_payload_directory_missing"));
            }
            continue;
        }
        let metadata = fs::symlink_metadata(&root).map_err(io_error)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("installed_payload_directory_unsafe"));
        }
        let actual = collect_regular_files(&root, &root)?;
        let relative = actual
            .into_iter()
            .map(|path| PathBuf::from(top).join(path))
            .collect::<BTreeSet<_>>();
        let selected = expected
            .iter()
            .filter(|path| path.starts_with(top))
            .cloned()
            .collect::<BTreeSet<_>>();
        if relative != selected {
            return Err(invalid("installed_payload_inventory_mismatch"));
        }
    }
    Ok(())
}

pub(super) fn verify_keg_file_inventory(
    keg: &Path,
    formula: &str,
    payload_paths: &BTreeSet<PathBuf>,
) -> Result<(), OrchestratorError> {
    let mut expected = payload_paths.clone();
    expected.insert(PathBuf::from("INSTALL_RECEIPT.json"));
    expected.insert(PathBuf::from("sbom.spdx.json"));
    expected.insert(PathBuf::from(".brew").join(format!("{formula}.rb")));
    let actual = collect_regular_files(keg, keg)?;
    let actual = actual.into_iter().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid("installed_keg_file_inventory_mismatch"));
    }
    Ok(())
}

pub(super) fn validate_brew_sbom(
    bytes: &[u8],
    formula: &str,
    version: &str,
    expected_asset: &str,
    expected_sha256: &str,
    expected_url: &str,
) -> Result<(), OrchestratorError> {
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(invalid("installed_brew_sbom_size"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("installed_brew_sbom_json"))?;
    let value = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| invalid("installed_brew_sbom_json"))?;
    validate_sbom_root(&value, formula, version)?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("installed_brew_sbom_shape"))?;
    validate_sbom_file(object, formula, expected_asset, expected_sha256)?;
    validate_sbom_packages(object, formula, version, expected_url, expected_sha256)
}

fn validate_sbom_root(
    value: &serde_json::Value,
    formula: &str,
    version: &str,
) -> Result<(), OrchestratorError> {
    use serde_json::Value;
    const ROOT_KEYS: &[&str] = &[
        "SPDXID",
        "spdxVersion",
        "name",
        "creationInfo",
        "dataLicense",
        "documentNamespace",
        "documentDescribes",
        "files",
        "packages",
        "relationships",
    ];
    let object = value
        .as_object()
        .ok_or_else(|| invalid("installed_brew_sbom_shape"))?;
    if object.len() != ROOT_KEYS.len()
        || !ROOT_KEYS.iter().all(|key| object.contains_key(*key))
        || object.get("SPDXID").and_then(Value::as_str) != Some("SPDXRef-DOCUMENT")
        || object.get("spdxVersion").and_then(Value::as_str) != Some("SPDX-2.3")
        || object.get("name").and_then(Value::as_str)
            != Some(format!("SBOM-SPDX-{formula}-{version}").as_str())
        || object.get("dataLicense").and_then(Value::as_str) != Some("CC0-1.0")
        || object.get("documentNamespace").and_then(Value::as_str)
            != Some(format!("https://formulae.brew.sh/spdx/{formula}-{version}.json").as_str())
    {
        return Err(invalid("installed_brew_sbom_identity"));
    }
    let creation = object
        .get("creationInfo")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("installed_brew_sbom_creation"))?;
    if creation.len() != 2
        || creation.get("created").and_then(Value::as_str) != Some("1970-01-01T00:00:00Z")
        || creation
            .get("creators")
            .and_then(Value::as_array)
            .is_none_or(|items| {
                items.len() != 1
                    || items[0].as_str() != Some("Tool: https://github.com/Homebrew/brew")
            })
    {
        return Err(invalid("installed_brew_sbom_creation"));
    }
    Ok(())
}

fn validate_sbom_file(
    object: &serde_json::Map<String, serde_json::Value>,
    formula: &str,
    expected_asset: &str,
    expected_sha256: &str,
) -> Result<(), OrchestratorError> {
    use serde_json::Value;
    let files = object
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("installed_brew_sbom_files"))?;
    if files.len() != 1 {
        return Err(invalid("installed_brew_sbom_files"));
    }
    let file = files[0]
        .as_object()
        .ok_or_else(|| invalid("installed_brew_sbom_files"))?;
    if file.len() != 3
        || file.get("SPDXID").and_then(Value::as_str)
            != Some(format!("SPDXRef-File-{formula}").as_str())
        || file.get("fileName").and_then(Value::as_str) != Some(expected_asset)
        || file
            .get("checksums")
            .and_then(Value::as_array)
            .is_none_or(|sum| {
                sum.len() != 1
                    || sum[0].get("algorithm").and_then(Value::as_str) != Some("SHA256")
                    || sum[0].get("checksumValue").and_then(Value::as_str) != Some(expected_sha256)
            })
    {
        return Err(invalid("installed_brew_sbom_file_identity"));
    }
    Ok(())
}

fn validate_sbom_packages(
    object: &serde_json::Map<String, serde_json::Value>,
    formula: &str,
    version: &str,
    expected_url: &str,
    expected_sha256: &str,
) -> Result<(), OrchestratorError> {
    use serde_json::Value;
    let packages = object
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("installed_brew_sbom_packages"))?;
    if packages.len() != 2
        || packages
            .iter()
            .filter_map(|p| p.get("name").and_then(Value::as_str))
            .collect::<BTreeSet<_>>()
            != [formula, "clang"].into_iter().collect()
        || !packages.iter().any(|p| {
            p.get("name").and_then(Value::as_str) == Some(formula)
                && p.get("versionInfo").and_then(Value::as_str) == Some(version)
                && p.get("downloadLocation").and_then(Value::as_str) == Some(expected_url)
                && p.get("checksums")
                    .and_then(Value::as_array)
                    .is_some_and(|sum| {
                        sum.len() == 1
                            && sum[0].get("algorithm").and_then(Value::as_str) == Some("SHA256")
                            && sum[0].get("checksumValue").and_then(Value::as_str)
                                == Some(expected_sha256)
                    })
        })
    {
        return Err(invalid("installed_brew_sbom_package_identity"));
    }
    Ok(())
}

pub(super) fn collect_regular_files(
    root: &Path,
    current: &Path,
) -> Result<Vec<PathBuf>, OrchestratorError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("installed_payload_symlink"));
        }
        if metadata.is_dir() {
            paths.extend(collect_regular_files(root, &path)?);
        } else if metadata.is_file() {
            paths.push(path.strip_prefix(root).map_err(io_error)?.to_path_buf());
        } else {
            return Err(invalid("installed_payload_special_file"));
        }
    }
    Ok(paths)
}
