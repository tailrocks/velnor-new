// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
mod drifts;
mod error;
pub mod io;
mod release;
mod reusable;
mod workflow_triggers;

pub use crate::release::{release_ruleset, release_ruleset_drift};
pub use drifts::{Drift, drift};
pub use error::GenerateError;

use std::collections::BTreeMap;
use std::io::Write;

use velnor_model::{
    FullRun, HelperRelease, HelperVersion, MbxStore, OnMain, PrCi, PushAfterMergeGroup, RepoFacts,
    RepoPath, RepoView,
};
use velnor_render::render;
use velnor_rust::{
    METADATA_PATH, check_required_check, check_tools, detect, metadata_file, plan,
    silent_build_scripts, source_roots_over_limit, suggest_groups as suggested_groups,
};

pub const NO_FULL_RUN_NOTE: &str = "on_main is \"affected\" and full_run is \"off\", so no run ever selects everything and a gap in selection would go unnoticed";
pub const COMPATIBLE_HELPER_SERIES: (u64, u64) = (0, 1);
pub const RELEASE_HELPER_MINIMUM: (u64, u64, u64) = (0, 1, 80);

pub fn generated_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    generated_with_notes(view).map(|(files, _)| files)
}

pub fn generated_with_notes(
    view: &RepoView,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    let (mut files, notes) = expected_with_notes(view)?;
    files.insert(
        METADATA_PATH,
        metadata_file(view).map_err(GenerateError::Metadata)?,
    );
    Ok((files, notes))
}

pub fn notes(view: &RepoView) -> Result<Vec<String>, GenerateError> {
    let config = detect(view)?.config;
    let blind = config.on_main == OnMain::Affected && config.full_run == FullRun::Off;
    Ok(blind
        .then(|| NO_FULL_RUN_NOTE.to_owned())
        .into_iter()
        .chain(
            silent_build_scripts(view)
                .iter()
                .map(velnor_rust::SilentBuildScript::note),
        )
        .collect())
}

pub fn suggest_groups(view: &RepoView) -> Result<(String, Vec<String>), GenerateError> {
    let facts = detect(view)?;
    Ok((suggested_groups(&facts), facts.timings_notes))
}

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    expected_with_notes(view).map(|(files, _)| files)
}

pub fn expected_with_notes(
    view: &RepoView,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    let facts = detect(view)?;
    let helper = check_helper(&facts)?;
    check_tools(&facts)?;
    if let Some(roots) = source_roots_over_limit(&facts) {
        return Err(GenerateError::TooManySourceRoots(roots));
    }
    let plan = plan(&facts);
    check_required_check(&facts, &plan)?;
    let mut files = render(&plan).map_err(GenerateError::Rules)?;
    files.extend(reusable::reusable_workflows(view, helper)?);
    let (release, release_notes) = release::release_workflow(view, &facts, helper, &files)?;
    files.extend(release);
    let mut notes = facts.timings_notes;
    notes.extend(facts.private_sources);
    notes.extend(facts.private_suggestion);
    notes.extend(release_notes);
    Ok((files, notes))
}

pub fn plan_generate(
    view: &RepoView,
    on_disk: &BTreeMap<RepoPath, Vec<u8>>,
    engine_configured: bool,
    out: &mut dyn Write,
) -> Result<(BTreeMap<RepoPath, String>, Vec<Drift>), GenerateError> {
    let (expected, notes) = generated_with_notes(view)?;
    for note in notes {
        writeln!(out, "velnor-gen: {note}").ok();
    }
    let drifts = drift(&expected, on_disk, engine_configured);
    Ok((expected, drifts))
}

fn check_helper(facts: &RepoFacts) -> Result<&HelperRelease, GenerateError> {
    let release = facts
        .helper
        .as_ref()
        .ok_or(GenerateError::HelperNotPinned)?;
    require_compatible(release.version())?;
    if (facts.config.on_main == OnMain::Everything || facts.config.full_run != FullRun::Off)
        && !release.version().at_least((0, 1, 81))
    {
        return Err(GenerateError::InputsBuildHelperTooOld(
            release.version().clone(),
        ));
    }
    if !facts.config.test_groups.is_empty() && !release.version().at_least((0, 1, 82)) {
        return Err(GenerateError::TestGroupsHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.pr_ci == PrCi::Label && !release.version().at_least((0, 1, 86)) {
        return Err(GenerateError::PrCiHelperTooOld(release.version().clone()));
    }
    if facts.config.crates_mirror.is_some() && !release.version().at_least((0, 1, 81)) {
        return Err(GenerateError::MirrorHelperTooOld(release.version().clone()));
    }
    if facts.config.private_sources.is_some() && !release.version().at_least((0, 1, 86)) {
        return Err(GenerateError::PrivateSourcesHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.push_after_merge_group == PushAfterMergeGroup::SkipWhenQueueRunPassed
        && !release.version().at_least((0, 1, 82))
    {
        return Err(GenerateError::MergeGroupHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.mbx_store == MbxStore::Shared && !release.version().at_least((0, 1, 85)) {
        return Err(GenerateError::MbxStoreHelperTooOld(
            release.version().clone(),
        ));
    }
    Ok(release)
}

pub fn require_compatible(version: &HelperVersion) -> Result<(), GenerateError> {
    if version.series() == Some(COMPATIBLE_HELPER_SERIES) {
        Ok(())
    } else {
        Err(GenerateError::IncompatibleHelper(version.clone()))
    }
}

pub fn check_tag(version: &str, tag: &str) -> Result<String, String> {
    let legacy = format!("rust-ci-v{version}");
    let current = format!("tools-v{version}");
    if tag == legacy || tag == current {
        Ok(format!("tag {tag} names version {version}"))
    } else {
        Err(format!(
            "tag {tag} must be {legacy} or {current}, because this program is version {version}"
        ))
    }
}
