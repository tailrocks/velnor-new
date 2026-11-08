// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, invalid};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

type Table = toml::map::Map<String, toml::Value>;

pub(super) fn workspace_manifest(
    source: &Path,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    git: &crate::crate_release_git::ReleaseGit,
    tracked: &BTreeSet<&str>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<toml::Value, OrchestratorError> {
    let metadata = crate::CrateSourceExecution::metadata(
        &crate::UnprivilegedSourceExecution,
        source,
        intent,
        git,
    )?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata).map_err(|_| invalid())?;
    let workspace = metadata
        .get("workspace_root")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid)?;
    let canonical_root = source.canonicalize().map_err(|_| invalid())?;
    let workspace = Path::new(workspace).canonicalize().map_err(|_| invalid())?;
    let manifest = workspace
        .strip_prefix(&canonical_root)
        .map_err(|_| invalid())?
        .join("Cargo.toml");
    let manifest = manifest.to_str().ok_or_else(invalid)?;
    read_manifest(source, manifest, tracked, deadline)
}

pub(super) fn manifest_features(
    source: &Path,
    manifest_path: &str,
    workspace: &toml::Value,
    tracked: &BTreeSet<&str>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BTreeMap<String, Vec<String>>, OrchestratorError> {
    let manifest = read_manifest(source, manifest_path, tracked, deadline)?;
    let root = manifest.as_table().ok_or_else(invalid)?;
    let mut retained = BTreeSet::new();
    collect_dependencies(root, workspace, &mut retained)?;
    if let Some(targets) = root.get("target") {
        for target in targets.as_table().ok_or_else(invalid)?.values() {
            collect_dependencies(
                target.as_table().ok_or_else(invalid)?,
                workspace,
                &mut retained,
            )?;
        }
    }
    let mut features = BTreeMap::new();
    if let Some(table) = root.get("features") {
        for (name, members) in table.as_table().ok_or_else(invalid)? {
            let mut members = members
                .as_array()
                .ok_or_else(invalid)?
                .iter()
                .map(|value| value.as_str().map(str::to_owned).ok_or_else(invalid))
                .collect::<Result<Vec<_>, _>>()?;
            members.retain(|member| {
                feature_dependency(member).is_none_or(|name| retained.contains(name))
            });
            members.sort();
            features.insert(name.clone(), members);
        }
    }
    Ok(features)
}

fn collect_dependencies(
    table: &Table,
    workspace: &toml::Value,
    retained: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    for section in ["dependencies", "build-dependencies", "build_dependencies"] {
        if let Some(dependencies) = table.get(section) {
            retained.extend(dependencies.as_table().ok_or_else(invalid)?.keys().cloned());
        }
    }
    for section in ["dev-dependencies", "dev_dependencies"] {
        if let Some(dependencies) = table.get(section) {
            for (name, dependency) in dependencies.as_table().ok_or_else(invalid)? {
                if versioned_dependency(name, dependency, workspace)? {
                    retained.insert(name.clone());
                }
            }
        }
    }
    Ok(())
}

fn versioned_dependency(
    name: &str,
    dependency: &toml::Value,
    workspace: &toml::Value,
) -> Result<bool, OrchestratorError> {
    if dependency.is_str() {
        return Ok(true);
    }
    let dependency = dependency.as_table().ok_or_else(invalid)?;
    if dependency.contains_key("version") {
        return Ok(true);
    }
    if dependency.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
        return Ok(false);
    }
    let inherited = workspace
        .get("workspace")
        .and_then(|value| value.get("dependencies"))
        .and_then(|value| value.get(name))
        .ok_or_else(invalid)?;
    Ok(inherited.is_str()
        || inherited
            .as_table()
            .is_some_and(|table| table.contains_key("version")))
}

fn feature_dependency(value: &str) -> Option<&str> {
    if let Some((name, _)) = value.split_once('/') {
        Some(name.strip_suffix('?').unwrap_or(name))
    } else {
        value.strip_prefix("dep:")
    }
}

fn read_manifest(
    source: &Path,
    manifest: &str,
    tracked: &BTreeSet<&str>,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<toml::Value, OrchestratorError> {
    let canonical_root = source.canonicalize().map_err(|_| invalid())?;
    let manifest_path = source
        .join(manifest)
        .canonicalize()
        .map_err(|_| invalid())?;
    let manifest_path = manifest_path
        .strip_prefix(&canonical_root)
        .map_err(|_| invalid())?
        .to_str()
        .ok_or_else(invalid)?;
    if !tracked.contains(manifest_path) {
        return Err(invalid());
    }
    let crate::safe_read::RepoBytes::Bytes(bytes) = crate::safe_read::read_repo_bytes_until(
        source,
        manifest_path,
        256 * 1024 * 1024,
        deadline,
    )?
    else {
        return Err(invalid());
    };
    toml::from_str(std::str::from_utf8(&bytes).map_err(|_| invalid())?).map_err(|_| invalid())
}
