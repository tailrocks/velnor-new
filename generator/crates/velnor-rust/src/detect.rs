// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fmt;

use velnor_model::{RepoFacts, RepoPath, RepoView, RustCiConfig, RustRoot, ToolPins};

use crate::config::{CONFIG_PATH, parse};

const MISE_CONFIGS: [RepoPath; 2] = [
    RepoPath::from_static("mise.toml"),
    RepoPath::from_static(".mise.toml"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    Config(String),
    NoRustRoot,
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(f, "{CONFIG_PATH}: {message}"),
            Self::NoRustRoot => f.write_str("no Cargo workspace found outside the excluded paths"),
        }
    }
}

impl std::error::Error for DetectError {}

#[must_use]
pub fn wanted_files(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    std::iter::once(CONFIG_PATH)
        .chain(MISE_CONFIGS)
        .filter(|path| tracked.contains(path))
        .collect()
}

pub fn metadata_roots(view: &RepoView) -> Result<Vec<RepoPath>, DetectError> {
    let config = config(view)?;
    let mut manifests: Vec<RepoPath> = tracked_manifests(&view.tracked_paths)
        .filter(|manifest| !excluded(&config, manifest))
        .cloned()
        .collect();
    manifests.sort_by_key(|manifest| (manifest.as_str().matches('/').count(), manifest.clone()));
    Ok(manifests)
}

pub fn detect(view: &RepoView) -> Result<RepoFacts, DetectError> {
    let config = config(view)?;
    let rust_roots: Vec<RustRoot> = view
        .rust_metadata
        .iter()
        .filter(|(manifest, metadata)| !metadata.members.is_empty() && !excluded(&config, manifest))
        .map(|(manifest, metadata)| {
            let mut packages: Vec<_> = metadata
                .members
                .iter()
                .map(|member| member.name.clone())
                .collect();
            packages.sort();
            RustRoot {
                manifest: manifest.clone(),
                packages,
            }
        })
        .collect();
    if rust_roots.is_empty() {
        return Err(DetectError::NoRustRoot);
    }
    let mise = MISE_CONFIGS
        .into_iter()
        .find(|path| view.files.contains_key(path));
    Ok(RepoFacts {
        rust_roots,
        tools: ToolPins { mise },
        config,
    })
}

fn config(view: &RepoView) -> Result<RustCiConfig, DetectError> {
    let config = view
        .files
        .get(&CONFIG_PATH)
        .map_or_else(|| Ok(RustCiConfig::default()), |bytes| parse(bytes))?;
    let manifests: Vec<&RepoPath> = tracked_manifests(&view.tracked_paths).collect();
    match config
        .exclude
        .iter()
        .find(|entry| !manifests.iter().any(|manifest| manifest.is_within(entry)))
    {
        Some(entry) => Err(DetectError::Config(format!(
            "exclude entry {entry} matches no tracked Cargo.toml"
        ))),
        None => Ok(config),
    }
}

fn excluded(config: &RustCiConfig, manifest: &RepoPath) -> bool {
    config
        .exclude
        .iter()
        .any(|excluded| manifest.is_within(excluded))
}

#[must_use]
pub fn manifests(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    tracked_manifests(tracked).cloned().collect()
}

fn tracked_manifests(tracked: &BTreeSet<RepoPath>) -> impl Iterator<Item = &RepoPath> {
    tracked
        .iter()
        .filter(|path| path.as_str() == "Cargo.toml" || path.as_str().ends_with("/Cargo.toml"))
}
