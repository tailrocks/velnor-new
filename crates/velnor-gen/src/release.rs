// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::workflow_triggers::{WorkflowFacts, workflow_facts};
use velnor_model::{
    HelperRelease, RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView, TargetKind,
};
use velnor_release::{
    ReleaseFiles, ReleasePlan, check_publish_budget, ruleset_drift, ruleset_json,
};
use velnor_render::{Owner, owner_of, render_release};
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
    let plan = read_plan(&release_files(view, Some(&reality))?)?;
    check_publish_budget(&plan.roots).map_err(GenerateError::Release)?;
    if !plan.pr_only_checks.is_empty() && !helper.version().at_least((0, 1, 82)) {
        return Err(GenerateError::PrOnlyChecksHelperTooOld(
            helper.version().clone(),
        ));
    }
    if plan.family.is_some() && !helper.version().at_least((0, 1, 83)) {
        return Err(GenerateError::FamilyTagHelperTooOld(
            helper.version().clone(),
        ));
    }
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
    let files = render_release(&plan, helper, &facts.config.default_branch)
        .map_err(GenerateError::Rules)?;
    Ok((files, Vec::new()))
}

fn read_plan(files: &ReleaseFiles) -> Result<ReleasePlan, GenerateError> {
    let plan = ReleasePlan::read(files).map_err(GenerateError::Release)?;
    plan.refuse_roots_below_git_root()
        .map_err(GenerateError::Release)?;
    Ok(plan)
}

pub fn release_ruleset(view: &RepoView) -> Result<String, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Err(GenerateError::NoReleaseConfig);
    }
    let plan = read_plan(&release_files(view, None)?)?;
    Ok(ruleset_json(&plan))
}

pub fn release_ruleset_drift(view: &RepoView, live: &str) -> Result<Vec<String>, GenerateError> {
    if !view.files.contains_key(&RELEASE_CONFIG_PATH) {
        return Err(GenerateError::NoReleaseConfig);
    }
    let plan = read_plan(&release_files(view, None)?)?;
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

fn tracked_workflows(view: &RepoView) -> impl Iterator<Item = (String, String)> + '_ {
    view.files
        .iter()
        .filter(|(path, _)| {
            let name = path.as_str();
            name.strip_prefix(".github/workflows/")
                .is_some_and(|relative| !relative.contains('/'))
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .filter(|(_, bytes)| owner_of(bytes) != Owner::VelnorGen)
        .map(|(path, bytes)| {
            (
                path.to_string(),
                String::from_utf8_lossy(bytes).into_owned(),
            )
        })
}

fn all_workflows(view: &RepoView, generated: &BTreeMap<RepoPath, String>) -> Vec<WorkflowFacts> {
    tracked_workflows(view)
        .chain(
            generated
                .iter()
                .map(|(path, text)| (path.to_string(), text.clone())),
        )
        .map(|(path, text)| workflow_facts(&path, &text))
        .collect()
}

fn workflow_names(workflows: &[WorkflowFacts]) -> BTreeSet<String> {
    workflows.iter().map(|flow| flow.name.clone()).collect()
}

fn push_workflows(workflows: &[WorkflowFacts]) -> BTreeSet<String> {
    workflows
        .iter()
        .filter(|flow| flow.pushes)
        .map(|flow| flow.name.clone())
        .collect()
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
    let workflows = reality.map(|known| all_workflows(view, known.generated));
    Ok(ReleaseFiles {
        release_toml: text(&RELEASE_CONFIG_PATH)?,
        release_plz_toml,
        mise_tomls,
        cargo_configs,
        toolchain_files,
        symlinks: view.symlinks.iter().map(ToString::to_string).collect(),
        rust_metadata: (!view.rust_metadata.is_empty()).then(|| view.rust_metadata.clone()),
        bin_targets: reality.map(|known| bin_targets(known.facts)),
        cargo_manifest_texts,
        cargo_manifests: reality.map(|known| cargo_manifests(known.facts)),
        workflow_names: workflows.as_deref().map(workflow_names),
        push_workflows: workflows.as_deref().map(push_workflows),
        mise_lock: text(&MISE_LOCK)?.unwrap_or_default(),
    })
}
