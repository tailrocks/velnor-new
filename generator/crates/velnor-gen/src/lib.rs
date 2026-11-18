// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub mod cli;
mod delta_cache;
mod drifts;
mod error;
pub mod io;
mod release;
mod workflow_triggers;

pub use crate::release::{release_ruleset, release_ruleset_drift};
pub use drifts::{Drift, drift};
pub use error::GenerateError;

use std::collections::BTreeMap;
use std::io::Write;

use velnor_model::{
    CacheKey, FullRun, HelperRelease, HelperVersion, OnMain, RepoFacts, RepoPath, RepoView,
    StepKind, WorkflowPlan,
};
use velnor_render::{MAX_WORKFLOW_BYTES, Rule, RuleViolation, render};
use velnor_rust::{
    METADATA_PATH, check_required_check, check_tools, detect, metadata_file, plan,
    plan_without_deltas, silent_build_scripts,
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

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    expected_with_notes(view).map(|(files, _)| files)
}

pub fn expected_with_notes(
    view: &RepoView,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    let facts = detect(view)?;
    let helper = check_helper(&facts)?;
    check_tools(&facts)?;
    let (plan, mut files, budget_notes) = plan_and_render(view, &facts)?;
    if plan
        .jobs
        .iter()
        .flat_map(|job| &job.steps)
        .any(|step| matches!(step.kind, StepKind::CacheSave(CacheKey::Delta { .. })))
        && let Some(cleanup) = velnor_render::delta_cleanup(helper, &plan.name)
    {
        files.insert(velnor_render::DELTA_CLEANUP_PATH, cleanup);
    }
    let (release, release_notes) = release::release_workflow(view, &facts, helper, &files)?;
    files.extend(release);
    let mut notes = facts.timings_notes;
    notes.extend(facts.private_sources);
    notes.extend(release_notes);
    notes.extend(budget_notes);
    Ok((files, notes))
}

type RenderedPlan = (WorkflowPlan, BTreeMap<RepoPath, String>, Vec<String>);

fn plan_and_render(view: &RepoView, facts: &RepoFacts) -> Result<RenderedPlan, GenerateError> {
    let full = plan(facts);
    delta_cache::validate_staging(view, &full)?;
    check_required_check(facts, &full)?;
    let violations = match render(&full) {
        Ok(files) => return Ok((full, files, Vec::new())),
        Err(violations) => violations,
    };
    let Some(bytes) = oversized(&violations) else {
        return Err(GenerateError::Rules(violations));
    };
    if !has_deltas(&full) {
        return Err(GenerateError::Rules(violations));
    }
    let reduced = plan_without_deltas(facts);
    delta_cache::validate_staging(view, &reduced)?;
    check_required_check(facts, &reduced)?;
    let files = render(&reduced).map_err(GenerateError::Rules)?;
    let note = format!(
        "the delta cache layer was left out because the workflow with it would be {bytes} bytes, above the {MAX_WORKFLOW_BYTES}-byte limit GitHub sets on a workflow file"
    );
    Ok((reduced, files, vec![note]))
}

fn oversized(violations: &[RuleViolation]) -> Option<usize> {
    let mut bytes = None;
    for violation in violations {
        let Rule::WorkflowTooLarge(size) = &violation.rule else {
            return None;
        };
        bytes = Some(*size);
    }
    bytes
}

fn has_deltas(plan: &WorkflowPlan) -> bool {
    plan.jobs.iter().flat_map(|job| &job.steps).any(|step| {
        matches!(
            step.kind,
            StepKind::CacheRestore(CacheKey::Delta { .. })
                | StepKind::CacheSave(CacheKey::Delta { .. })
        )
    })
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
    if facts.config.crates_mirror.is_some() && !release.version().at_least((0, 1, 81)) {
        return Err(GenerateError::MirrorHelperTooOld(release.version().clone()));
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
    let expected = format!("rust-ci-v{version}");
    if tag == expected {
        Ok(format!("tag {tag} names version {version}"))
    } else {
        Err(format!(
            "tag {tag} must be {expected}, because this program is version {version}"
        ))
    }
}
