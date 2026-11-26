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
    let Some((package, feature)) = name.split_once('/') else {
        let root_only = plain_names_need_root_package(view, &root.manifest);
        return root
            .members
            .iter()
            .filter(|member| !root_only || member.manifest == root.manifest)
            .any(|member| defined_features(view, member).contains(name));
    };
    match root
        .members
        .iter()
        .find(|member| member.name.as_str() == package)
    {
        Some(member) => defined_features(view, member).contains(feature),
        None => root
            .members
            .iter()
            .filter_map(|member| manifest(view, member))
            .any(|manifest| feature_dependency_specs(&manifest).any(|(key, _)| key == package)),
    }
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
