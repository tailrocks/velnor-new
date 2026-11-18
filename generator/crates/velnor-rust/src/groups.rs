// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use velnor_model::{
    HelperRelease, RepoPath, RepoView, RootSlug, RustCiConfig, RustRoot, Shape, WorkspaceGroup,
};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

const ROOT_MANIFEST: RepoPath = RepoPath::from_static("Cargo.toml");
const GROUP_HELPER: (u64, u64, u64) = (0, 1, 82);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGroup {
    name: String,
    roots: Vec<String>,
}

pub(crate) fn parse(groups: Vec<RawGroup>) -> Result<Vec<WorkspaceGroup>, Reason> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut parsed = Vec::new();
    for group in groups {
        let name = group.name.trim().to_owned();
        if !valid_name(&name) {
            return Err(Reason::InvalidValue {
                key: "workspace_groups.name",
                value: group.name.clone(),
            });
        }
        if !names.insert(group.name.clone()) {
            return Err(Reason::DuplicateGroup(name));
        }
        if group.roots.len() < 2 {
            return Err(Reason::InvalidValue {
                key: "workspace_groups.roots",
                value: name,
            });
        }
        let mut seen: BTreeSet<RepoPath> = BTreeSet::new();
        let mut roots = Vec::new();
        for entry in &group.roots {
            let manifest = manifest_of(entry)?;
            if !seen.insert(manifest.clone()) {
                return Err(Reason::GroupRootTwice {
                    name: name.clone(),
                    root: manifest,
                });
            }
            roots.push(manifest);
        }
        let slug = RootSlug::new(name.clone()).map_err(|_| Reason::InvalidValue {
            key: "workspace_groups.name",
            value: name.clone(),
        })?;
        let gate = RootSlug::new(format!("group_{name}")).map_err(|_| Reason::InvalidValue {
            key: "workspace_groups.name",
            value: name.clone(),
        })?;
        parsed.push(WorkspaceGroup {
            name,
            slug,
            gate,
            roots,
        });
    }
    Ok(parsed)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn manifest_of(entry: &str) -> Result<RepoPath, Reason> {
    let entry = entry.trim().trim_end_matches('/');
    if entry == "." {
        return Ok(ROOT_MANIFEST);
    }
    RepoPath::new(format!("{entry}/Cargo.toml")).map_err(|_| Reason::InvalidValue {
        key: "workspace_groups.roots",
        value: entry.to_owned(),
    })
}

pub(crate) fn check(
    view: &RepoView,
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    let invalid = |reason| DetectError::invalid(&CONFIG_PATH, reason);
    let mut claimed: BTreeMap<&RepoPath, &str> = BTreeMap::new();
    for group in &config.workspace_groups {
        check_target_dir(view, group, rust_roots)?;
        for manifest in &group.roots {
            if !rust_roots.iter().any(|root| &root.manifest == manifest) {
                return Err(invalid(Reason::UnknownGroupRoot(manifest.clone())));
            }
            if claimed.insert(manifest, &group.name).is_some() {
                return Err(invalid(Reason::GroupRootTwice {
                    name: group.name.clone(),
                    root: manifest.clone(),
                }));
            }
        }
        if rust_roots
            .iter()
            .any(|root| RootSlug::of(&root.manifest).as_str() == group.name)
        {
            return Err(invalid(Reason::GroupNameTaken(group.name.clone())));
        }
    }
    Ok(())
}

fn check_target_dir(
    view: &RepoView,
    group: &WorkspaceGroup,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    let folder = group.target_dir();
    let overlaps = |path: &RepoPath| {
        path == &folder
            || path.as_str().starts_with(&format!("{folder}/"))
            || folder.as_str().starts_with(&format!("{path}/"))
    };
    if view.tracked_paths.iter().any(overlaps)
        || view.rust_metadata.keys().any(overlaps)
        || rust_roots.iter().any(|root| holds_root(&folder, root))
    {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::TargetDirOverlap(folder.to_string()),
        ));
    }
    Ok(())
}

fn holds_root(folder: &RepoPath, root: &RustRoot) -> bool {
    std::iter::successors(root.manifest.parent(), RepoPath::parent)
        .any(|ancestor| ancestor == *folder)
}

pub(crate) fn check_shape(config: &RustCiConfig) -> Result<(), DetectError> {
    if !config.workspace_groups.is_empty() && config.shape == Some(Shape::BuildThenGroups) {
        return Err(DetectError::invalid(&CONFIG_PATH, Reason::GroupShape));
    }
    Ok(())
}

pub(crate) fn check_helper(
    config: &RustCiConfig,
    helper: Option<&HelperRelease>,
) -> Result<(), DetectError> {
    if config.workspace_groups.is_empty()
        || helper.is_some_and(|release| release.version().at_least(GROUP_HELPER))
    {
        return Ok(());
    }
    Err(DetectError::invalid(
        &crate::lock::LOCK_PATH,
        Reason::GroupHelper,
    ))
}
