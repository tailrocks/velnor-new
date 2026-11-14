// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RepoFacts, RepoPath, RepoView, RustCiConfig};

use crate::config::{CONFIG_PATH, excluded, parse};
use crate::error::{DetectError, Reason};
use crate::lock::{LOCK_PATH, parse_lock};
use crate::mise::{MISE_CONFIGS, MISE_LOCK, tool_pins};
use crate::roots::{CARGO_LOCK, is_cargo_config, rust_roots};
use crate::toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, toolchain};

const CARGO_MANIFEST: &str = "Cargo.toml";

#[must_use]
pub fn wanted_files(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    [
        CONFIG_PATH,
        LOCK_PATH,
        MISE_LOCK,
        TOOLCHAIN_PLAIN,
        TOOLCHAIN_TOML,
    ]
    .into_iter()
    .chain(MISE_CONFIGS.iter().cloned())
    .filter(|path| tracked.contains(path))
    .chain(tracked.iter().filter(|path| is_cargo_config(path)).cloned())
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
    let rust_roots = rust_roots(view, &config)?;
    let toolchain = toolchain(&view.files)?;
    let tools = tool_pins(&view.files)?;
    let helper = view
        .files
        .get(&LOCK_PATH)
        .map(|bytes| {
            parse_lock(bytes)
                .map_err(|error| DetectError::invalid(&LOCK_PATH, Reason::HelperLock(error)))
        })
        .transpose()?;
    Ok(RepoFacts {
        rust_roots,
        toolchain,
        tools,
        config,
        helper,
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
        .find(|entry| !manifests.iter().any(|manifest| entry.matches(manifest)))
    {
        Some(entry) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnmatchedExclude(entry.clone()),
        )),
        None => Ok(config),
    }
}

#[must_use]
pub fn cargo_files(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    tracked
        .iter()
        .filter(|path| is_named(path, CARGO_MANIFEST) || is_named(path, CARGO_LOCK.as_str()))
        .cloned()
        .collect()
}

fn tracked_manifests(tracked: &BTreeSet<RepoPath>) -> impl Iterator<Item = &RepoPath> {
    tracked.iter().filter(|path| is_named(path, CARGO_MANIFEST))
}

fn is_named(path: &RepoPath, name: &str) -> bool {
    path.as_str() == name
        || path
            .as_str()
            .strip_suffix(name)
            .is_some_and(|rest| rest.ends_with('/'))
}
