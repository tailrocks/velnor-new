// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{DependencyKind, RepoPath, RepoView, RustPackage};

use crate::harness::enabled_features;
use crate::toml_file::table;

pub(crate) fn requested_by_siblings(view: &RepoView, package: &RustPackage) -> Vec<String> {
    let Some((workspace, metadata)) = view.rust_metadata.iter().find(|(_, metadata)| {
        metadata
            .members
            .iter()
            .any(|member| member.manifest == package.manifest)
    }) else {
        return Vec::new();
    };
    let split_build = resolver_splits_build(view, workspace);
    let manifests: BTreeMap<&RepoPath, toml::Table> = metadata
        .members
        .iter()
        .filter_map(|member| {
            let parsed = table(view.files.get(&member.manifest)?).ok()?;
            Some((&member.manifest, parsed))
        })
        .collect();
    let mut requested: BTreeMap<&RepoPath, BTreeSet<String>> = BTreeMap::new();
    loop {
        let mut changed = false;
        for member in &metadata.members {
            let own = manifests.get(&member.manifest);
            let enabled = own.map(|parsed| {
                let asked: Vec<String> = requested
                    .get(&member.manifest)
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect();
                enabled_features(parsed, &asked)
            });
            for dependency in &member.dependencies {
                let Some(target) = dependency
                    .manifest
                    .as_ref()
                    .filter(|manifest| **manifest != member.manifest)
                    .and_then(|manifest| manifests.get_key_value(manifest))
                    .map(|(manifest, _)| *manifest)
                else {
                    continue;
                };
                if split_build && dependency.kind == DependencyKind::Build {
                    continue;
                }
                let mut wanted = dependency.features.clone();
                if let (Some(parsed), Some(enabled)) = (own, &enabled) {
                    let mut keys =
                        dependency_keys(parsed, dependency.name.as_str(), dependency.kind);
                    keys.extend(dependency.rename.clone());
                    if keys.is_empty() {
                        keys.insert(dependency.name.to_string());
                    }
                    wanted.extend(forwarded_features(parsed, enabled, &keys));
                }
                let slot = requested.entry(target).or_default();
                for feature in wanted {
                    changed |= slot.insert(feature);
                }
            }
        }
        if !changed {
            break;
        }
    }
    requested
        .remove(&package.manifest)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn resolver_splits_build(view: &RepoView, workspace: &RepoPath) -> bool {
    let Some(manifest) = view
        .files
        .get(workspace)
        .and_then(|bytes| table(bytes).ok())
    else {
        return false;
    };
    let text = |section: &str, key: &str| {
        manifest
            .get(section)
            .and_then(|value| value.get(key))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    match text("workspace", "resolver").or_else(|| text("package", "resolver")) {
        Some(resolver) => resolver != "1",
        None => text("package", "edition").is_some_and(|edition| edition.as_str() >= "2021"),
    }
}

fn dependency_keys(
    manifest: &toml::Table,
    package: &str,
    kind: DependencyKind,
) -> BTreeSet<String> {
    let section = match kind {
        DependencyKind::Normal => "dependencies",
        DependencyKind::Dev => "dev-dependencies",
        DependencyKind::Build => "build-dependencies",
    };
    let targets = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::values)
        .filter_map(toml::Value::as_table);
    std::iter::once(manifest)
        .chain(targets)
        .filter_map(|scope| scope.get(section)?.as_table())
        .flat_map(toml::Table::iter)
        .filter(|(key, spec)| {
            spec.get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(key)
                == package
        })
        .map(|(key, _)| key.clone())
        .collect()
}

fn forwarded_features(
    manifest: &toml::Table,
    enabled: &BTreeSet<String>,
    keys: &BTreeSet<String>,
) -> Vec<String> {
    manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(name, _)| enabled.contains(*name))
        .filter_map(|(_, members)| members.as_array())
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter_map(|member| member.split_once('/'))
        .filter(|(name, _)| keys.contains(name.trim_end_matches('?')))
        .map(|(_, feature)| feature.to_owned())
        .collect()
}
