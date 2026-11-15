// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView};
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
    let plan = ReleasePlan::read(&release_files(view)?).map_err(GenerateError::Release)?;
    render_release(&plan, facts.config.runner.as_str(), helper).map_err(GenerateError::Rules)
}

pub fn release_ruleset(view: &RepoView) -> Result<String, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Err(GenerateError::NoReleaseConfig);
    }
    let plan = ReleasePlan::read(&release_files(view)?).map_err(GenerateError::Release)?;
    Ok(ruleset_json(&plan))
}

fn release_files(view: &RepoView) -> Result<ReleaseFiles, GenerateError> {
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
    let mut mise_toml = None;
    for config in &MISE_CONFIGS {
        if mise_toml.is_none() {
            mise_toml = text(config)?;
        }
    }
    Ok(ReleaseFiles {
        release_toml: text(&RELEASE_CONFIG_PATH)?,
        release_plz_toml,
        mise_toml: mise_toml.unwrap_or_default(),
        mise_lock: text(&MISE_LOCK)?.unwrap_or_default(),
    })
}
