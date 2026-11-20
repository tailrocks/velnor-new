// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod advice;
pub mod cli;
mod delta_cache;
mod drifts;
mod error;
mod helper_gate;
pub mod io;
mod plan_render;
mod release;
mod reusable;
mod workflow_triggers;

pub use crate::release::{release_ruleset, release_ruleset_drift};
pub use advice::{NO_FULL_RUN_NOTE, SPARSE_SUGGESTION_BYTES, notes, sparse_note, suggest_groups};
pub use drifts::{Drift, drift};
pub use error::GenerateError;
use helper_gate::check_helper;
pub use helper_gate::require_compatible;
pub use velnor_workflows::HELPER_COMMANDS;

use std::collections::BTreeMap;
use std::io::Write;

use velnor_model::{CacheKey, RepoPath, RepoView, StepKind};
use velnor_rust::{METADATA_PATH, check_tools, detect, metadata_file, source_roots_over_limit};

use crate::plan_render::plan_and_render;

pub const COMPATIBLE_HELPER_SERIES: (u64, u64) = (0, 1);
pub const HELPER_MINIMUM: (u64, u64, u64) = (0, 1, 81);
pub const TAG_PUSH_HELPER: (u64, u64, u64) = (0, 1, 98);
pub const BUDGET_HELPER: (u64, u64, u64) = (0, 1, 92);
/// The first release whose `velnor-ci` accepts the `test_lane` key (and runs `lane-guard`).
pub const TEST_LANE_HELPER: (u64, u64, u64) = (0, 1, 93);
/// The first release whose `velnor-ci` accepts `job` in a `[[test_lane]]` table.
// First released in tools-v0.1.107 (not tagged yet when this gate was written).
pub const TEST_LANE_JOB_HELPER: (u64, u64, u64) = (0, 1, 107);
pub const SPARSE_HINT_HELPER: (u64, u64, u64) = (0, 1, 103);

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

pub fn expected_files(view: &RepoView) -> Result<BTreeMap<RepoPath, String>, GenerateError> {
    expected_with_notes(view).map(|(files, _)| files)
}

pub fn expected_with_notes(
    view: &RepoView,
) -> Result<(BTreeMap<RepoPath, String>, Vec<String>), GenerateError> {
    let facts = detect(view)?;
    let excluded = velnor_rust::excluded_lock_findings(view)?;
    if let Some(finding) = excluded.iter().find(|finding| finding.refusal) {
        return Err(GenerateError::ExcludedLockRun(finding.clone()));
    }
    let helper = check_helper(&facts)?;
    check_tools(&facts)?;
    if let Some(roots) = source_roots_over_limit(&facts) {
        return Err(GenerateError::TooManySourceRoots(roots));
    }
    let (plan, mut files, budget_notes) = plan_and_render(view, &facts)?;
    files.extend(reusable::reusable_workflows(view, helper)?);
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
    notes.extend(facts.private_suggestion);
    notes.extend(release_notes);
    notes.extend(budget_notes);
    notes.extend(
        excluded
            .iter()
            .map(velnor_rust::ExcludedLockFinding::message),
    );
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
