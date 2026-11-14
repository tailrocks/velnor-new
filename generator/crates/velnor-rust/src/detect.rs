// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{PackageName, RepoFacts, RepoPath, RepoView, RustCiConfig, RustRoot};

use crate::config::{CONFIG_PATH, excluded, parse};
use crate::error::{DetectError, Reason};
use crate::lock::{LOCK_PATH, parse_lock};
use crate::mise::{MISE_CONFIGS, MISE_LOCK, tool_pins};
use crate::reads::{INPUTS_LOCK, parse_inputs_lock};
use crate::roots::{CARGO_LOCK, is_cargo_config, rust_roots};
use crate::select::root_slug;
use crate::timings::{TIMINGS_PATH, parse_timings};
use crate::toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, toolchain};
use crate::unowned::unowned;

const CARGO_MANIFEST: &str = "Cargo.toml";

#[must_use]
pub fn wanted_files(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    [
        CONFIG_PATH,
        LOCK_PATH,
        INPUTS_LOCK,
        TIMINGS_PATH,
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
    let (rust_roots, targets) = rust_roots(view, &config)?;
    let unowned = unowned(view, &rust_roots, &targets);
    let mut slugs = BTreeMap::new();
    for root in &rust_roots {
        if let Some(first) = slugs.insert(root_slug(&root.manifest), &root.manifest) {
            return Err(DetectError::SameSlug {
                first: first.clone(),
                second: root.manifest.clone(),
            });
        }
    }
    let compile_inputs = view
        .files
        .get(&INPUTS_LOCK)
        .map_or_else(|| Ok(BTreeMap::new()), |bytes| parse_inputs_lock(bytes))?;
    let named = config.inputs.iter().flat_map(|input| &input.packages);
    if let Some(package) = unknown_package(&rust_roots, named) {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnknownInputPackage(package),
        ));
    }
    if let Some(package) = unknown_package(&rust_roots, compile_inputs.values().flatten()) {
        return Err(DetectError::invalid(
            &INPUTS_LOCK,
            Reason::UnknownLockedPackage(package),
        ));
    }
    let toolchain = toolchain(&view.files)?;
    let tools = tool_pins(&view.files)?;
    check_setup_files(view, &rust_roots)?;
    let helper = view
        .files
        .get(&LOCK_PATH)
        .map(|bytes| {
            parse_lock(bytes)
                .map_err(|error| DetectError::invalid(&LOCK_PATH, Reason::HelperLock(error)))
        })
        .transpose()?;
    let timings = view
        .files
        .get(&TIMINGS_PATH)
        .map_or_else(|| Ok(BTreeMap::new()), |bytes| parse_timings(bytes))?;
    if let Some(root) = timings
        .keys()
        .find(|root| !rust_roots.iter().any(|known| known.manifest == **root))
    {
        return Err(DetectError::invalid(
            &TIMINGS_PATH,
            Reason::UnknownRoot(root.clone()),
        ));
    }
    Ok(RepoFacts {
        timings,
        rust_roots,
        toolchain,
        tools,
        config,
        helper,
        compile_inputs,
        unowned,
    })
}

fn check_setup_files(view: &RepoView, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    if let Some(file) = view
        .tracked_paths
        .iter()
        .find(|path| other_mise_config(path))
    {
        return Err(DetectError::invalid(file, Reason::OtherMiseConfig));
    }
    let folders: Vec<RepoPath> = rust_roots
        .iter()
        .filter_map(|root| root.manifest.parent())
        .flat_map(|folder| std::iter::successors(Some(folder), RepoPath::parent))
        .collect();
    let nested = view
        .tracked_paths
        .iter()
        .find(|path| {
            folders.iter().any(|folder| {
                path.as_str()
                    .strip_prefix(&format!("{folder}/"))
                    .and_then(|rest| RepoPath::new(rest).ok())
                    .is_some_and(|rest| {
                        other_mise_config(&rest)
                            || MISE_CONFIGS.contains(&rest)
                            || rest == TOOLCHAIN_PLAIN
                            || rest == TOOLCHAIN_TOML
                    })
            })
        })
        .cloned();
    if let Some(file) = nested {
        return Err(DetectError::NestedToolchain { file });
    }
    Ok(())
}

const MISE_FOLDERS: [&str; 3] = [".config/", ".mise/", "mise/"];

fn other_mise_config(path: &RepoPath) -> bool {
    let text = path.as_str();
    let toml = std::path::Path::new(text)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"));
    let named = |name: &str| name.starts_with("mise") || name.starts_with(".mise");
    let at_root = !text.contains('/') && named(text) && toml;
    let in_folder = MISE_FOLDERS.iter().any(|folder| {
        text.strip_prefix(folder)
            .is_some_and(|rest| toml && (*folder != ".config/" || named(rest)))
    });
    text == ".tool-versions"
        || (at_root && !MISE_CONFIGS.iter().any(|config| config == path))
        || in_folder
}

fn unknown_package<'p>(
    roots: &[RustRoot],
    mut packages: impl Iterator<Item = &'p PackageName>,
) -> Option<String> {
    packages
        .find(|package| {
            !roots
                .iter()
                .flat_map(|root| &root.members)
                .any(|member| member.name == **package)
        })
        .map(ToString::to_string)
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

#[must_use]
pub fn manifests(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    tracked_manifests(tracked).cloned().collect()
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
