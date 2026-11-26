// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    DependencyKind, RepoPath, RepoView, RustPackage, Target, TargetKind, TargetName,
};

use crate::harness::{dependency_specs, enabled_features, implicit_features};
use crate::toml_file::table;

pub(crate) fn requested_by_siblings(
    view: &RepoView,
    package: &RustPackage,
    workspace_features: &[String],
) -> Vec<String> {
    let Some((workspace, metadata)) = view.rust_metadata.iter().find(|(_, metadata)| {
        metadata
            .members
            .iter()
            .any(|member| member.manifest == package.manifest)
    }) else {
        return Vec::new();
    };
    let split_build = resolver_splits_build(view, workspace);
    let root_only = plain_names_need_root_package(view, workspace);
    let manifests: BTreeMap<&RepoPath, toml::Table> = metadata
        .members
        .iter()
        .filter_map(|member| {
            let parsed = table(view.files.get(&member.manifest)?).ok()?;
            Some((&member.manifest, parsed))
        })
        .collect();
    let mut requested: BTreeMap<&RepoPath, BTreeSet<String>> = metadata
        .members
        .iter()
        .filter_map(|member| {
            let parsed = manifests.get(&member.manifest)?;
            Some((
                &member.manifest,
                workspace_requests(
                    parsed,
                    member.name.as_str(),
                    workspace_features,
                    !root_only || member.manifest == *workspace,
                ),
            ))
        })
        .collect();
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
                    if dependency.optional
                        && !keys.iter().any(|key| activated(parsed, enabled, key))
                    {
                        continue;
                    }
                    wanted.extend(forwarded_features(parsed, enabled, &keys));
                    if !root_only || member.manifest == *workspace {
                        wanted.extend(workspace_forwards(workspace_features, &keys));
                    }
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

pub(crate) fn gated_targets_built(
    view: &RepoView,
    package: &RustPackage,
    workspace_features: &[String],
) -> Vec<(TargetName, TargetKind)> {
    if workspace_features.is_empty() {
        return Vec::new();
    }
    let Some(manifest) = view
        .files
        .get(&package.manifest)
        .and_then(|bytes| table(bytes).ok())
    else {
        return Vec::new();
    };
    let closure = |names: &[String]| {
        enabled_features(&manifest, &requested_by_siblings(view, package, names))
    };
    let enabled = closure(workspace_features);
    let before = closure(&[]);
    let built = |features: &BTreeSet<String>, target: &Target| {
        target
            .required_features
            .iter()
            .all(|required| features.contains(&required.name))
    };
    package
        .targets
        .iter()
        .filter(|target| {
            target.requires_features && built(&enabled, target) && !built(&before, target)
        })
        .flat_map(|target| target.kinds.iter().map(|kind| (target.name.clone(), *kind)))
        .collect()
}

fn workspace_requests(
    manifest: &toml::Table,
    package: &str,
    names: &[String],
    plain: bool,
) -> BTreeSet<String> {
    let defined: BTreeSet<&str> = manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::keys)
        .map(String::as_str)
        .collect();
    let implicit = implicit_features(manifest);
    names
        .iter()
        .flat_map(|name| match name.split_once('/') {
            Some((owner, feature)) => [
                (owner == package).then_some(feature),
                (plain && implicit.contains(owner)).then_some(owner),
            ],
            None => [plain.then_some(name.as_str()), None],
        })
        .flatten()
        .filter(|name| defined.contains(name) || implicit.contains(*name))
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn plain_names_need_root_package(view: &RepoView, workspace: &RepoPath) -> bool {
    !resolver_splits_build(view, workspace)
        && view
            .files
            .get(workspace)
            .and_then(|bytes| table(bytes).ok())
            .is_some_and(|manifest| manifest.contains_key("package"))
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
        None => edition(&manifest).is_some_and(|edition| edition.as_str() >= "2021"),
    }
}

fn edition(manifest: &toml::Table) -> Option<String> {
    let declared = manifest.get("package")?.get("edition")?;
    let inherited = declared
        .get("workspace")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let value = if inherited {
        manifest.get("workspace")?.get("package")?.get("edition")?
    } else {
        declared
    };
    value.as_str().map(str::to_owned)
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

fn workspace_forwards(names: &[String], keys: &BTreeSet<String>) -> Vec<String> {
    names
        .iter()
        .filter_map(|name| name.split_once('/'))
        .filter(|(owner, _)| keys.contains(*owner))
        .map(|(_, feature)| feature.to_owned())
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
        .filter(|(name, _)| {
            let key = name.trim_end_matches('?');
            keys.contains(key) && (key == *name || activated(manifest, enabled, key))
        })
        .map(|(_, feature)| feature.to_owned())
        .collect()
}

fn activated(manifest: &toml::Table, enabled: &BTreeSet<String>, key: &str) -> bool {
    let optional = dependency_specs(manifest).any(|(name, spec)| {
        name == key && spec.get("optional").and_then(toml::Value::as_bool) == Some(true)
    });
    let explicit = format!("dep:{key}");
    !optional
        || enabled.contains(key)
        || manifest
            .get("features")
            .and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
            .filter(|(name, _)| enabled.contains(*name))
            .filter_map(|(_, members)| members.as_array())
            .flatten()
            .filter_map(toml::Value::as_str)
            .any(|member| member == explicit)
}
