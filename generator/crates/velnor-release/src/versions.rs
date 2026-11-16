// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RustMetadata};

use crate::git_ref;
use crate::members::{inherited, inherited_tag, owner};
use crate::plan::read_all;
use crate::tag::{VERSION, is_tag_name};

pub type Versions = BTreeMap<(String, String), String>;

pub fn check_release_versions(
    files: &[(String, String)],
    metadata: &BTreeMap<RepoPath, RustMetadata>,
    versions: &Versions,
) -> Result<(), String> {
    released_tags(files, metadata, versions).map(drop)
}

pub fn released_tags(
    files: &[(String, String)],
    metadata: &BTreeMap<RepoPath, RustMetadata>,
    versions: &Versions,
) -> Result<BTreeMap<String, String>, String> {
    let mut tags = BTreeMap::new();
    for config in read_all(files)? {
        let Some((workspace, known)) = owner(&config.root, metadata) else {
            continue;
        };
        let listed = config
            .packages
            .iter()
            .filter(|package| config.releases(&package.name))
            .map(|package| (package.name.clone(), package.tag_pattern.clone()));
        let unlisted = inherited(&config, known).filter_map(|member| {
            let name = member.name.as_str();
            inherited_tag(&config, known, name).map(|pattern| (name.to_owned(), pattern))
        });
        for (name, pattern) in listed.chain(unlisted) {
            let version = versions
                .get(&(workspace.as_str().to_owned(), name.clone()))
                .ok_or_else(|| {
                    format!("{}: no version is known for package {name}", config.root)
                })?;
            let tag = pattern.as_str().replace(VERSION, version);
            if !is_tag_name(&tag) || !git_ref::is_valid(&format!("refs/tags/{tag}")) {
                return Err(format!(
                    "{}: package {name} at version {version} would be tagged {tag}, which git cannot create after the release is published",
                    config.root
                ));
            }
            if let Some(other) = tags.insert(name.clone(), tag.clone()) {
                return Err(format!(
                    "{}: package {name} is released as {other} and as {tag}",
                    config.root
                ));
            }
        }
    }
    Ok(tags)
}
