// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_model::{DependencyKind, EventKind, RepoFacts, RepoPath, RepoView, TargetKind};
use velnor_rust::{detect, lint_runs};

use crate::facts::{TestResult, lib, package, path, repo};
use crate::pipeline::pin_helper;

pub(crate) const CONFIG: &str =
    "[[lint]]\nkind = 'generated-check'\nfrom_source = true\ngate_on_plan = true\n";
pub(crate) const GEN: &str = "crates/velnor-gen/Cargo.toml";
pub(crate) const ENGINE: &str = "crates/engine/Cargo.toml";
pub(crate) const CORE: &str = "crates/core/Cargo.toml";
pub(crate) const OTHER: &str = "crates/other/Cargo.toml";
pub(crate) const PINNED: &str = "0.1.117";

const LOCK: &str = "shared/borrowed.rs\tcore\tcrates/core/Cargo.toml\ndocs/embedded.md\tcore\tcrates/core/Cargo.toml\ndata/dir\tcore\tcrates/core/Cargo.toml\nshared/other.rs\tother\tcrates/other/Cargo.toml\ndata/other.md\tother\tcrates/other/Cargo.toml\n";
const REGISTRY_LOCK: &str = "version = 4\n\n[[package]]\nname = \"fnv\"\nversion = \"1.0.7\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"sum\"\n";

pub(crate) fn manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n")
}

pub(crate) fn world() -> Result<RepoView, Box<dyn Error>> {
    let members = vec![
        package(
            "velnor-gen",
            GEN,
            &[("engine", DependencyKind::Normal, ENGINE)],
            &[("velnor-gen", TargetKind::Bin, false)],
        )?,
        package(
            "engine",
            ENGINE,
            &[("core", DependencyKind::Normal, CORE)],
            &[("engine", TargetKind::Lib, true)],
        )?,
        lib("core", CORE)?,
        lib("other", OTHER)?,
    ];
    let manifests: Vec<(String, String)> = [
        ("velnor-gen", GEN),
        ("engine", ENGINE),
        ("core", CORE),
        ("other", OTHER),
    ]
    .iter()
    .map(|(name, file)| ((*file).to_owned(), manifest(name)))
    .collect();
    let mut files: Vec<(&str, &str)> = vec![
        (".velnor/rust-ci.toml", CONFIG),
        (".velnor/rust-inputs.lock", LOCK),
        ("Cargo.lock", REGISTRY_LOCK),
    ];
    files.extend(
        manifests
            .iter()
            .map(|(file, text)| (file.as_str(), text.as_str())),
    );
    let mut view = repo(vec![("Cargo.toml", members)], &files)?;
    pin_helper(&mut view, PINNED)?;
    Ok(view)
}

pub(crate) fn put(view: &mut RepoView, file: &str, text: &str) -> TestResult {
    view.tracked_paths.insert(path(file)?);
    view.files.insert(path(file)?, text.as_bytes().to_vec());
    Ok(())
}

pub(crate) fn set_config(view: &mut RepoView, config: &str) -> TestResult {
    put(view, ".velnor/rust-ci.toml", config)
}

pub(crate) fn paths(files: &[&str]) -> Result<BTreeSet<RepoPath>, Box<dyn Error>> {
    files
        .iter()
        .map(|file| RepoPath::new(*file).map_err(Into::into))
        .collect()
}

pub(crate) fn decide(
    facts: &RepoFacts,
    tracked: &BTreeSet<RepoPath>,
    changed: &[&str],
    unplain: &[&str],
    links: &[&str],
) -> Result<bool, Box<dyn Error>> {
    lint_runs(
        facts,
        EventKind::PullRequest,
        Some(&paths(changed)?),
        tracked,
        &paths(links)?,
        &BTreeMap::new(),
        &paths(unplain)?,
    )
    .into_iter()
    .find(|(slug, _)| slug.as_str() == "generated_check")
    .map(|(_, run)| run)
    .ok_or_else(|| "no plan decision for the generated check".into())
}

pub(crate) fn decide_in_place(
    facts: &RepoFacts,
    tracked: &BTreeSet<RepoPath>,
    file: &str,
) -> Result<bool, Box<dyn Error>> {
    let base = BTreeMap::from([(RepoPath::new(file)?, "before\n".to_owned())]);
    lint_runs(
        facts,
        EventKind::PullRequest,
        Some(&paths(&[file])?),
        tracked,
        &BTreeSet::new(),
        &base,
        &BTreeSet::new(),
    )
    .into_iter()
    .find(|(slug, _)| slug.as_str() == "generated_check")
    .map(|(_, run)| run)
    .ok_or_else(|| "no plan decision for the generated check".into())
}

pub(crate) fn runs(view: &RepoView, changed: &[&str]) -> Result<bool, Box<dyn Error>> {
    decide(&detect(view)?, &view.tracked_paths, changed, &[], &[])
}

pub(crate) fn runs_every(view: &RepoView, files: &[&str]) -> Result<bool, Box<dyn Error>> {
    let facts = detect(view)?;
    for file in files {
        if !decide(&facts, &view.tracked_paths, &[file], &[], &[])? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn skips_every(view: &RepoView, files: &[&str]) -> Result<bool, Box<dyn Error>> {
    let facts = detect(view)?;
    for file in files {
        if decide(&facts, &view.tracked_paths, &[file], &[], &[])? {
            return Ok(false);
        }
    }
    Ok(true)
}
