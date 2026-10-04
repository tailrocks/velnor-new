//! Strict relocation of modern Cargo path package IDs and skipped path edges.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::normalize_posix_path;
use velnor_actions_rust::WorkspaceRecord;

const ROOT_URI: &str = "path+file://velnor-checkout";
const ROOT_PATH: &str = "velnor-checkout:";

pub(super) fn normalize(
    root: &Path,
    inventories: &[(String, WorkspaceRecord)],
) -> Result<Vec<(String, WorkspaceRecord)>, String> {
    relocate(root, inventories, true)
}

pub(super) fn rehydrate(
    root: &Path,
    inventories: &[(String, WorkspaceRecord)],
) -> Result<Vec<(String, WorkspaceRecord)>, String> {
    relocate(root, inventories, false)
}

fn relocate(
    root: &Path,
    inventories: &[(String, WorkspaceRecord)],
    normalize: bool,
) -> Result<Vec<(String, WorkspaceRecord)>, String> {
    let root = root.canonicalize().map_err(|err| err.to_string())?;
    let root = root.to_str().ok_or("analysis_inventory_root_non_utf8")?;
    let uri = format!("path+file://{}", uri_path(root));
    let mut candidates = BTreeSet::new();
    let mut result = Vec::with_capacity(inventories.len());
    for (manifest, record) in inventories {
        check_relative(manifest, false)?;
        if !candidates.insert(manifest) {
            return Err("analysis_inventory_duplicate_candidate".to_owned());
        }
        let mut record = record.clone();
        check_relative(&record.workspace_root, true)?;
        let (from, to) = if normalize {
            (uri.as_str(), ROOT_URI)
        } else {
            (ROOT_URI, uri.as_str())
        };
        for member in &mut record.members {
            *member = relocate_id(member, from, to)?;
        }
        for package in &mut record.packages {
            if package.external {
                return Err("analysis_inventory_external_package".to_owned());
            }
            check_relative(&package.manifest, false)?;
            package.id = relocate_id(&package.id, from, to)?;
            let directory = Path::new(&package.manifest)
                .parent()
                .unwrap_or(Path::new(""));
            let directory = directory
                .to_str()
                .ok_or("analysis_inventory_path_non_utf8")?;
            let expected = if directory.is_empty() {
                to.to_owned()
            } else {
                format!("{to}/{}", uri_path(directory))
            };
            if package.id.split_once('#').map(|(path, _)| path) != Some(expected.as_str()) {
                return Err("analysis_inventory_package_manifest_mismatch".to_owned());
            }
        }
        for edge in &mut record.edges {
            edge.from = relocate_id(&edge.from, from, to)?;
            edge.to = relocate_id(&edge.to, from, to)?;
        }
        for edge in &mut record.skipped_edges {
            edge.from = relocate_id(&edge.from, from, to)?;
            edge.path = relocate_skipped(&edge.path, root, normalize)?;
        }
        validate_record(manifest, &record)?;
        result.push((manifest.clone(), record));
    }
    Ok(result)
}

fn relocate_id(id: &str, from: &str, to: &str) -> Result<String, String> {
    let suffix = id
        .strip_prefix(from)
        .ok_or("analysis_inventory_unrooted_package_id")?;
    let (path, package) = suffix
        .split_once('#')
        .ok_or("analysis_inventory_invalid_package_id")?;
    if !path.is_empty() {
        let relative = path
            .strip_prefix('/')
            .ok_or("analysis_inventory_package_root_boundary")?;
        if relative.is_empty() || relative.split('/').any(|part| part == ".." || part == ".") {
            return Err("analysis_inventory_invalid_package_path".to_owned());
        }
    }
    if package.is_empty() || package.contains('#') || package.chars().any(char::is_whitespace) {
        return Err("analysis_inventory_invalid_package_id".to_owned());
    }
    Ok(format!("{to}{suffix}"))
}

fn relocate_skipped(path: &str, root: &str, normalize: bool) -> Result<String, String> {
    if normalize {
        if Path::new(path).is_absolute() {
            let relative = Path::new(path)
                .strip_prefix(root)
                .map_err(|_| "analysis_inventory_external_skipped_path")?;
            let relative = relative
                .to_str()
                .ok_or("analysis_inventory_path_non_utf8")?;
            check_relative(relative, true)?;
            return Ok(format!("{ROOT_PATH}{relative}"));
        }
        check_relative(path, true)?;
        Ok(path.to_owned())
    } else if let Some(relative) = path.strip_prefix(ROOT_PATH) {
        check_relative(relative, true)?;
        Ok(Path::new(root)
            .join(relative)
            .to_string_lossy()
            .into_owned())
    } else {
        check_relative(path, true)?;
        Ok(path.to_owned())
    }
}

fn check_relative(path: &str, allow_root: bool) -> Result<(), String> {
    if path.is_empty() && allow_root {
        return Ok(());
    }
    let canonical = normalize_posix_path(path).map_err(|err| err.to_string())?;
    if canonical != path
        || path.is_empty()
        || path.contains(':')
        || path.contains('\0')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("analysis_inventory_noncanonical_path".to_owned());
    }
    Ok(())
}

fn validate_record(manifest: &str, record: &WorkspaceRecord) -> Result<(), String> {
    let packages: BTreeSet<_> = record
        .packages
        .iter()
        .map(|package| package.id.as_str())
        .collect();
    let manifests: BTreeSet<_> = record
        .packages
        .iter()
        .map(|package| package.manifest.as_str())
        .collect();
    let members: BTreeSet<_> = record.members.iter().map(String::as_str).collect();
    if packages.len() != record.packages.len()
        || manifests.len() != record.packages.len()
        || members.len() != record.members.len()
        || !members.is_subset(&packages)
        || (!manifests.contains(manifest)
            && manifest != crate::discover::workspace_manifest(&record.workspace_root))
        || record
            .packages
            .iter()
            .any(|package| package.in_workspace != members.contains(package.id.as_str()))
        || record.edges.iter().any(|edge| {
            !packages.contains(edge.from.as_str()) || !packages.contains(edge.to.as_str())
        })
        || record
            .skipped_edges
            .iter()
            .any(|edge| !packages.contains(edge.from.as_str()))
    {
        return Err("analysis_inventory_inconsistent_record".to_owned());
    }
    Ok(())
}

fn uri_path(path: &str) -> String {
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~' | b':') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
