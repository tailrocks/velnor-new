// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RepoView, RustCiConfig, RustMember, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::harness::{feature_dependency_specs, implicit_features};
use crate::sibling_features::plain_names_need_root_package;
use crate::toml_file::table;

pub(crate) fn check(
    view: &RepoView,
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    let workspace = &config.workspace_features.names;
    if let Some(lane) = config.test_lanes.iter().find(|lane| {
        let features = &lane.features;
        !features.names.is_empty()
            && !features.all
            && !features.no_default
            && features.names.iter().all(|name| workspace.contains(name))
    }) {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::TestLaneDuplicatesWorkspaceFeatures {
                features: lane.features.names.join(","),
            },
        ));
    }
    for root in rust_roots {
        let missing = config
            .workspace_features
            .names
            .iter()
            .find(|name| !root_accepts(view, root, name));
        if let Some(feature) = missing {
            return Err(DetectError::invalid(
                &CONFIG_PATH,
                Reason::UnknownWorkspaceFeature {
                    root: root
                        .manifest
                        .parent()
                        .map_or_else(|| ".".to_owned(), |folder| folder.to_string()),
                    feature: feature.clone(),
                },
            ));
        }
    }
    Ok(())
}

fn root_accepts(view: &RepoView, root: &RustRoot, name: &str) -> bool {
    let root_only = plain_names_need_root_package(view, &root.manifest);
    let confined = || {
        root.members
            .iter()
            .filter(move |member| !root_only || member.manifest == root.manifest)
    };
    let Some((package, feature)) = name.split_once('/') else {
        return confined().any(|member| defined_features(view, member).contains(name));
    };
    let aliased: Vec<Option<String>> = confined()
        .filter_map(|member| Some((member, manifest(view, member)?)))
        .flat_map(|(member, manifest)| {
            feature_dependency_specs(&manifest)
                .filter(|(key, _)| *key == package)
                .map(|(_, spec)| dependency_directory(view, root, member, spec, package))
                .collect::<Vec<_>>()
        })
        .collect();
    if aliased.is_empty() {
        return root.members.iter().any(|member| {
            member.name.as_str() == package && defined_features(view, member).contains(feature)
        });
    }
    aliased.iter().all(|directory| {
        directory.as_ref().is_none_or(|directory| {
            root.members
                .iter()
                .find(|member| member_directory(member) == *directory)
                .is_none_or(|member| defined_features(view, member).contains(feature))
        })
    })
}

fn manifest(view: &RepoView, member: &RustMember) -> Option<toml::Table> {
    view.files
        .get(&member.manifest)
        .and_then(|bytes| table(bytes).ok())
}

fn defined_features(view: &RepoView, member: &RustMember) -> BTreeSet<String> {
    let Some(manifest) = manifest(view, member) else {
        return BTreeSet::new();
    };
    manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(toml::Table::keys)
        .cloned()
        .chain(implicit_features(&manifest))
        .collect()
}

fn member_directory(member: &RustMember) -> String {
    member
        .directory
        .as_ref()
        .map_or_else(String::new, ToString::to_string)
}

fn dependency_directory(
    view: &RepoView,
    root: &RustRoot,
    member: &RustMember,
    spec: &toml::Value,
    key: &str,
) -> Option<String> {
    let inherited = spec.get("workspace").and_then(toml::Value::as_bool) == Some(true);
    let (path, base) = if inherited {
        let shared = view
            .files
            .get(&root.manifest)
            .and_then(|bytes| table(bytes).ok())?;
        let path = shared
            .get("workspace")?
            .get("dependencies")?
            .get(key)?
            .get("path")?
            .as_str()?
            .to_owned();
        (path, root.manifest.parent())
    } else {
        (
            spec.get("path")?.as_str()?.to_owned(),
            member.manifest.parent(),
        )
    };
    let mut parts: Vec<String> = base.map_or_else(Vec::new, |base| {
        base.as_str().split('/').map(ToOwned::to_owned).collect()
    });
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_owned()),
        }
    }
    Some(parts.join("/"))
}
