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
) -> Result<(), DetectError> {
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
    for root in roots {
        let (used, source) = target_dir_source(&read, root)?;
        let (rendered, _) = target_dir_source(&generated, root)?;
        if let (true, Some(file)) = (used != rendered, source) {
            return Err(DetectError::invalid(
                file,
                Reason::RuntimeTargetDir { used, rendered },
            ));
        }
    }
    Ok(())
}
