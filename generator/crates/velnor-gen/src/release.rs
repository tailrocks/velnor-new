// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView};
use velnor_release::{ReleaseFiles, ReleasePlan};
use velnor_render::render_release;
use velnor_rust::{MISE_CONFIGS, MISE_LOCK, reject_release_credential_hooks};

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
    if facts
        .toolchain
        .as_ref()
        .is_some_and(|toolchain| toolchain.path.is_some())
    {
        return Err(GenerateError::ReleaseToolchainPath);
    }
    let plan = ReleasePlan::read(&release_files(view)?).map_err(GenerateError::Release)?;
    for root in &plan.roots {
        let manifest = if root.root == "." {
            "Cargo.toml".to_owned()
        } else {
            format!("{}/Cargo.toml", root.root)
        };
        let detected = facts
            .rust_roots
            .iter()
            .find(|detected| detected.manifest.as_str() == manifest)
            .ok_or_else(|| GenerateError::ReleaseRootWithoutCargoToml(root.root.clone()))?;
        reject_release_credential_hooks(view, &detected.manifest)?;
        if let Some((package, _)) = root.packages.iter().find(|(package, _)| {
            !detected
                .members
                .iter()
                .any(|member| member.name.as_str() == package)
        }) {
            return Err(GenerateError::ReleasePackageNotInRoot {
                root: root.root.clone(),
                package: package.clone(),
            });
        }
    }
    render_release(&plan, helper, &facts.config.default_branch).map_err(GenerateError::Rules)
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
    let [primary, dot] = &MISE_CONFIGS;
    let (mise_toml, dot_mise_toml) = (text(primary)?, text(dot)?);
    Ok(ReleaseFiles {
        release_toml: text(&RELEASE_CONFIG_PATH)?,
        release_plz_toml,
        mise_toml: mise_toml.unwrap_or_default(),
        dot_mise_toml,
        mise_lock: text(&MISE_LOCK)?.unwrap_or_default(),
    })
}
