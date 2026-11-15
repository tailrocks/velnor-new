// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView, TargetKind,
};
use velnor_release::{ReleaseFiles, ReleasePlan, ruleset_json};
use velnor_render::render_release;
use velnor_rust::{MISE_CONFIGS, MISE_LOCK};

use crate::GenerateError;

pub(crate) fn release_workflow(
    view: &RepoView,
    facts: &RepoFacts,
) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Ok(BTreeMap::new());
    }
    let helper = facts
        .helper
        .as_ref()
        .ok_or(GenerateError::ReleaseNeedsHelper)?;
    let plan =
        ReleasePlan::read(&release_files(view, Some(facts))?).map_err(GenerateError::Release)?;
    render_release(&plan, facts.config.runner.as_str(), helper).map_err(GenerateError::Rules)
}

pub fn release_ruleset(view: &RepoView) -> Result<String, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Err(GenerateError::NoReleaseConfig);
    }
    let plan = ReleasePlan::read(&release_files(view, None)?).map_err(GenerateError::Release)?;
    Ok(ruleset_json(&plan))
}

fn bin_targets(facts: &RepoFacts) -> BTreeSet<(String, String)> {
    facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .flat_map(|member| {
            member
                .targets
                .iter()
                .filter(|target| {
                    target.kinds.contains(&TargetKind::Bin) && !target.requires_features
                })
                .map(|target| (member.name.to_string(), target.name.to_string()))
        })
        .collect()
}

fn release_files(
    view: &RepoView,
    facts: Option<&RepoFacts>,
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
    Ok(ReleaseFiles {
        release_toml: text(&RELEASE_CONFIG_PATH)?,
        release_plz_toml,
        mise_tomls,
        cargo_configs,
        bin_targets: facts.map(bin_targets),
        mise_lock: text(&MISE_LOCK)?.unwrap_or_default(),
    })
}
