// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RepoPath, RepoView, RustRoot, UnownedPackage};

use crate::cargo_flags::flag_tokens;
use crate::detect_checks::generator_root;
use crate::generator_inputs::closure_manifests;
use crate::generator_lock;
use crate::roots::{is_cargo_config, patch_files};
use crate::toml_file::table;
use crate::weaken::COMPILER_KEYS;

const REDIRECTING_SOURCE_KEYS: [&str; 3] = ["directory", "local-registry", "git"];
const TARGET_PROGRAM_KEYS: [&str; 2] = ["runner", "linker"];
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
        || generator_lock::unmodelled(view, &root.manifest)
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
        || names_a_program(config)
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

fn names_a_program(config: &toml::Table) -> bool {
    let build = config.get("build").and_then(toml::Value::as_table);
    let targets: Vec<&toml::Table> = config
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values().filter_map(toml::Value::as_table))
        .collect();
    build.is_some_and(|build| COMPILER_KEYS.iter().any(|key| build.contains_key(*key)))
        || targets.iter().any(|target| {
            TARGET_PROGRAM_KEYS
                .iter()
                .any(|key| target.contains_key(*key))
        })
        || build
            .into_iter()
            .chain(targets)
            .filter_map(|section| section.get("rustflags"))
            .any(|flags| sets_a_linker(&flag_tokens(flags)))
}

fn sets_a_linker(tokens: &[String]) -> bool {
    tokens.iter().enumerate().any(|(index, token)| {
        token.starts_with("-Clinker=")
            || token.starts_with("--codegen=linker=")
            || (matches!(token.as_str(), "-C" | "--codegen")
                && tokens
                    .get(index + 1)
                    .is_some_and(|next| next.starts_with("linker=")))
    })
}
