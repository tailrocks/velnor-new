// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RepoView, RustRoot, UnownedPackage};

use crate::detect_checks::generator_root;
use crate::generator_inputs::closure_manifests;
use crate::roots::{CARGO_LOCK, is_cargo_config, patch_files};
use crate::toml_file::table;

const REGISTRIES: [&str; 2] = ["registry+", "sparse+"];
const REDIRECTING_SOURCE_KEYS: [&str; 3] = ["directory", "local-registry", "git"];
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

pub(crate) fn unmodelled(
    view: &RepoView,
    roots: &[RustRoot],
    unowned: &BTreeMap<RepoPath, UnownedPackage>,
) -> bool {
    let Some(root) = generator_root(roots) else {
        return false;
    };
    redirected(view, &root.manifest)
        || off_registry(view, &root.manifest)
        || closure_manifests(roots, unowned)
            .iter()
            .any(|manifest| picks_a_build(view, manifest))
}

fn redirected(view: &RepoView, manifest: &RepoPath) -> bool {
    patch_files(view, manifest)
        .into_iter()
        .filter(|(file, _, _)| is_cargo_config(file))
        .any(|(_, bytes, _)| table(bytes).is_ok_and(|config| redirects(&config)))
}

fn redirects(config: &toml::Table) -> bool {
    config.contains_key("paths")
        || config
            .get("source")
            .and_then(toml::Value::as_table)
            .is_some_and(|sources| {
                sources.values().any(|source| {
                    REDIRECTING_SOURCE_KEYS
                        .iter()
                        .any(|key| source.get(key).is_some())
                })
            })
}

fn off_registry(view: &RepoView, manifest: &RepoPath) -> bool {
    let Some(lock) = view
        .files
        .get(&manifest.sibling(&CARGO_LOCK))
        .and_then(|bytes| table(bytes).ok())
    else {
        return true;
    };
    lock.get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| package.get("source")?.as_str())
        .any(|source| !REGISTRIES.iter().any(|scheme| source.starts_with(scheme)))
}

fn picks_a_build(view: &RepoView, manifest: &RepoPath) -> bool {
    let Some(parsed) = view.files.get(manifest).and_then(|bytes| table(bytes).ok()) else {
        return true;
    };
    let features = parsed
        .get("features")
        .and_then(toml::Value::as_table)
        .is_some_and(|features| !features.is_empty());
    let platforms = parsed
        .get("target")
        .and_then(toml::Value::as_table)
        .is_some_and(|targets| {
            targets.values().any(|target| {
                DEPENDENCY_TABLES
                    .iter()
                    .any(|section| target.get(section).is_some())
            })
        });
    features || platforms
}
