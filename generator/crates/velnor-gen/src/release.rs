// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::yaml_scalar::scalar;
use velnor_model::{
    HelperRelease, RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView, TargetKind,
};
use velnor_release::{ReleaseFiles, ReleasePlan, ruleset_drift, ruleset_json};
use velnor_render::{Owner, owner_of, render_release, unfinalized_without_binary};
use velnor_rust::{MISE_CONFIGS, MISE_LOCK, reject_release_credential_hooks};

use crate::GenerateError;

pub(crate) fn release_workflow(
    view: &RepoView,
    facts: &RepoFacts,
    helper: &HelperRelease,
    generated: &BTreeMap<RepoPath, String>,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Ok((BTreeMap::new(), Vec::new()));
    }
    if facts
        .toolchain
        .as_ref()
        .is_some_and(|toolchain| toolchain.path.is_some())
    {
        return Err(GenerateError::ReleaseToolchainPath);
    }
    let reality = Reality { facts, generated };
    let plan =
        ReleasePlan::read(&release_files(view, Some(&reality))?).map_err(GenerateError::Release)?;
    for root in &plan.roots {
        let manifest = if root.root == "." {
            "Cargo.toml".to_owned()
        } else {
            format!("{}/Cargo.toml", root.root)
        };
        if let Some(detected) = facts
            .rust_roots
            .iter()
            .find(|detected| detected.manifest.as_str() == manifest)
        {
            reject_release_credential_hooks(view, &detected.manifest)?;
        }
    }
    let unfinalized = unfinalized_without_binary(&plan, helper);
    let notes = if unfinalized.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "WARNING: helper {} cannot publish the draft release of {}, which has no [[binaries]] entry, so that release stays a draft; pin velnor-ci 0.1.81 or newer",
            helper.version(),
            unfinalized.into_iter().collect::<Vec<_>>().join(", ")
        )]
    };
    let files = render_release(&plan, helper, &facts.config.default_branch)
        .map_err(GenerateError::Rules)?;
    Ok((files, notes))
}

pub fn release_ruleset(view: &RepoView) -> Result<String, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Err(GenerateError::NoReleaseConfig);
    }
    let plan = ReleasePlan::read(&release_files(view, None)?).map_err(GenerateError::Release)?;
    Ok(ruleset_json(&plan))
}

pub fn release_ruleset_drift(view: &RepoView, live: &str) -> Result<Vec<String>, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Err(GenerateError::NoReleaseConfig);
    }
    let plan = ReleasePlan::read(&release_files(view, None)?).map_err(GenerateError::Release)?;
    Ok(ruleset_drift(&plan, live))
}

struct Reality<'a> {
    facts: &'a RepoFacts,
    generated: &'a BTreeMap<RepoPath, String>,
}

fn cargo_manifests(facts: &RepoFacts) -> BTreeSet<String> {
    facts
        .rust_roots
        .iter()
        .flat_map(|root| {
            std::iter::once(root.manifest.to_string()).chain(
                root.members
                    .iter()
                    .map(|member| member.manifest.to_string()),
            )
        })
        .collect()
}

fn bin_targets(facts: &RepoFacts) -> BTreeSet<(String, String, String)> {
    let mut found = BTreeSet::new();
    for root in &facts.rust_roots {
        for member in &root.members {
            for target in &member.targets {
                if target.kinds.contains(&TargetKind::Bin) && !target.requires_features {
                    for manifest in [&root.manifest, &member.manifest] {
                        found.insert((
                            manifest.to_string(),
                            member.name.to_string(),
                            target.name.to_string(),
                        ));
                    }
                }
            }
        }
    }
    found
}

fn workflow_name(text: &str) -> Option<String> {
    let value = text.lines().find_map(|line| line.strip_prefix("name:"))?;
    scalar(value)
}

fn workflow_names(view: &RepoView, generated: &BTreeMap<RepoPath, String>) -> BTreeSet<String> {
    let tracked = view
        .files
        .iter()
        .filter(|(path, _)| {
            let name = path.as_str();
            name.starts_with(".github/workflows/")
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .filter(|(_, bytes)| owner_of(bytes) != Owner::VelnorGen)
        .filter_map(|(_, bytes)| workflow_name(&String::from_utf8_lossy(bytes)));
    let made = generated.values().filter_map(|text| workflow_name(text));
    tracked.chain(made).collect()
}

fn release_files(
    view: &RepoView,
    reality: Option<&Reality<'_>>,
) -> Result<ReleaseFiles, GenerateError> {
    let text = |path: &RepoPath| -> Result<Option<String>, GenerateError> {
        view.files
            .get(path)
            .map(|bytes| {
                String::from_utf8(bytes.clone())
                    .map_err(|_| GenerateError::ReleaseFileNotUtf8(path.clone()))
            })
            .transpose()
    };
    let mut release_plz_toml = BTreeMap::new();
    for path in view.files.keys() {
        if path.as_str().rsplit('/').next() == Some(RELEASE_PLZ_FILE) {
            let root = path
                .parent()
                .map_or_else(|| ".".to_owned(), |p| p.to_string());
            release_plz_toml.insert(root, text(path)?.unwrap_or_default());
        }
    }
    let mut mise_tomls = Vec::new();
    for config in &MISE_CONFIGS {
        mise_tomls.extend(text(config)?);
    }
    let mut cargo_configs = BTreeMap::new();
    for path in view.files.keys() {
        let name = path.as_str();
        if name == ".cargo/config.toml"
            || name == ".cargo/config"
            || name.ends_with("/.cargo/config.toml")
            || name.ends_with("/.cargo/config")
        {
            cargo_configs.insert(name.to_owned(), text(path)?.unwrap_or_default());
        }
    }
    let mut cargo_manifest_texts = BTreeMap::new();
    for path in view.files.keys() {
        let name = path.as_str();
        if name == "Cargo.toml" || name.ends_with("/Cargo.toml") {
            cargo_manifest_texts.insert(name.to_owned(), text(path)?.unwrap_or_default());
        }
    }
    let mut toolchain_files = BTreeMap::new();
    for path in view.files.keys() {
        if matches!(
            path.as_str().rsplit('/').next(),
            Some("rust-toolchain" | "rust-toolchain.toml")
        ) {
            toolchain_files.insert(path.to_string(), text(path)?.unwrap_or_default());
        }
    }
    Ok(ReleaseFiles {
        release_toml: text(&RELEASE_CONFIG_PATH)?,
        release_plz_toml,
        mise_tomls,
        cargo_configs,
        toolchain_files,
        symlinks: view.symlinks.iter().map(ToString::to_string).collect(),
        rust_metadata: reality.map(|_| view.rust_metadata.clone()),
        bin_targets: reality.map(|known| bin_targets(known.facts)),
        cargo_manifest_texts,
        cargo_manifests: reality.map(|known| cargo_manifests(known.facts)),
        workflow_names: reality.map(|known| workflow_names(view, known.generated)),
        mise_lock: text(&MISE_LOCK)?.unwrap_or_default(),
    })
}
