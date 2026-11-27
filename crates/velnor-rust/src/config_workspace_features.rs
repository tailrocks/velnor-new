// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, RustCiConfig, RustMember, RustRoot};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};
use crate::harness::gated_harness_target;
use crate::sibling_features::plain_names_need_root_package;
use crate::toml_file::table;

pub(crate) fn check(
    view: &RepoView,
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    let workspace = &config.workspace_features.names;
    if workspace.is_empty() {
        return Ok(());
    }
    if let Some(lane) = config.test_lanes.iter().find(|lane| {
        let features = &lane.features;
        !features.names.is_empty()
            && !features.all
            && !features.no_default
            && features.names.iter().all(|name| workspace.contains(name))
    }) {
        return Err(invalid(Reason::TestLaneDuplicatesWorkspaceFeatures {
            features: lane.features.names.join(","),
        }));
    }
    for root in rust_roots {
        let folder = root
            .manifest
            .parent()
            .map_or_else(|| ".".to_owned(), |folder| folder.to_string());
        if let Some(feature) = workspace
            .iter()
            .find(|name| !root_defines(view, root, name))
        {
            return Err(invalid(Reason::UnknownWorkspaceFeature {
                root: folder,
                feature: feature.clone(),
            }));
        }
        for member in &root.members {
            let gated = manifest(view, member)
                .and_then(|parsed| gated_harness_target(&parsed, member.name.as_str()));
            if let Some(target) = gated {
                return Err(invalid(Reason::GatedHarnessWithWorkspaceFeatures {
                    package: member.name.to_string(),
                    target,
                }));
            }
        }
    }
    Ok(())
}

fn invalid(reason: Reason) -> DetectError {
    DetectError::invalid(&CONFIG_PATH, reason)
}

fn root_defines(view: &RepoView, root: &RustRoot, name: &str) -> bool {
    let root_only = plain_names_need_root_package(view, &root.manifest);
    root.members
        .iter()
        .filter(|member| !root_only || member.manifest == root.manifest)
        .filter_map(|member| manifest(view, member))
        .any(|parsed| {
            parsed
                .get("features")
                .and_then(toml::Value::as_table)
                .is_some_and(|features| features.contains_key(name))
        })
}

fn manifest(view: &RepoView, member: &RustMember) -> Option<toml::Table> {
    view.files
        .get(&member.manifest)
        .and_then(|bytes| table(bytes).ok())
}
