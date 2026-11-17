// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoPath, RepoView};

use crate::error::{DetectError, Reason};
use crate::roots::effective_configs;
use crate::target_dir::target_dir_source;
use crate::toml_file::table;

const CONFIG_FILES: [&str; 2] = [".cargo/config", ".cargo/config.toml"];

#[must_use]
pub fn cargo_config_paths(roots: &[RepoPath]) -> Vec<RepoPath> {
    let mut folders: BTreeSet<Option<RepoPath>> = BTreeSet::from([None]);
    for root in roots {
        let mut folder = root.parent();
        while let Some(current) = folder {
            folder = current.parent();
            folders.insert(Some(current));
        }
    }
    folders
        .iter()
        .flat_map(|folder| {
            CONFIG_FILES.iter().filter_map(move |name| {
                let name = RepoPath::new(*name).ok()?;
                Some(
                    folder
                        .as_ref()
                        .map_or_else(|| name.clone(), |folder| folder.child(name.as_str())),
                )
            })
        })
        .collect()
}

pub fn repo_profile(files: &BTreeMap<RepoPath, Vec<u8>>) -> Result<String, DetectError> {
    let mut sections = Vec::new();
    for (file, bytes) in effective_configs(files) {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        if let Some(profile) = parsed.get("profile") {
            let wrapped = toml::Table::from_iter([("profile".to_owned(), profile.clone())]);
            sections.push(format!("{file}\n{wrapped}\n"));
        }
    }
    Ok(sections.concat())
}

pub fn check_runtime_configs(
    roots: &[RepoPath],
    files: &BTreeMap<RepoPath, Vec<u8>>,
    tracked: &BTreeSet<RepoPath>,
) -> Result<BTreeSet<RepoPath>, DetectError> {
    for (file, bytes) in effective_configs(files) {
        if let Ok(parsed) = table(bytes)
            && parsed.contains_key("include")
        {
            return Err(DetectError::invalid(file, Reason::CargoConfigInclude));
        }
    }
    let read = RepoView {
        files: files.clone(),
        ..RepoView::default()
    };
    let generated = RepoView {
        files: files
            .iter()
            .filter(|(file, _)| tracked.contains(*file))
            .map(|(file, bytes)| (file.clone(), bytes.clone()))
            .collect(),
        ..RepoView::default()
    };
    let mut targets = BTreeSet::new();
    for root in roots {
        let (used, source) = target_dir_source(&read, root)?;
        let (rendered, rendered_from) = target_dir_source(&generated, root)?;
        if used != rendered {
            return Err(DetectError::invalid(
                &blamed(source, rendered_from, &read, root),
                Reason::RuntimeTargetDir { used, rendered },
            ));
        }
        targets.insert(used);
    }
    Ok(targets)
}

fn blamed(
    source: Option<&RepoPath>,
    rendered_from: Option<&RepoPath>,
    read: &RepoView,
    root: &RepoPath,
) -> RepoPath {
    source.cloned().unwrap_or_else(|| {
        rendered_from.map_or_else(
            || root.clone(),
            |from| {
                shadowing(from, read)
                    .cloned()
                    .unwrap_or_else(|| from.clone())
            },
        )
    })
}

fn shadowing<'a>(rendered_from: &RepoPath, read: &'a RepoView) -> Option<&'a RepoPath> {
    let bare = rendered_from.as_str().strip_suffix(".toml")?;
    read.files.keys().find(|file| file.as_str() == bare)
}
