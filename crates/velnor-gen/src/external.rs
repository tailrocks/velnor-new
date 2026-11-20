// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use velnor_model::{RepoPath, RepoView};
use velnor_render::{Owner, owner_of};
use velnor_rust::detect;

use crate::{Drift, ExternalWorkflowError, GenerateError};

/// The `external_workflows` paths of `.velnor/rust-ci.toml` that `velnor-gen check` skips.
/// Each must exist on disk, must not be a file velnor-gen writes, and must not carry the
/// velnor-gen header (a file it owns but no longer generates is stale, not external).
pub(crate) fn skipped(
    view: &RepoView,
    expected: &BTreeMap<RepoPath, String>,
    on_disk: &BTreeMap<RepoPath, Vec<u8>>,
) -> Result<BTreeSet<RepoPath>, GenerateError> {
    let facts = detect(view)?;
    let Some(paths) = facts.config.external_workflows else {
        return Ok(BTreeSet::new());
    };
    for path in &paths {
        if expected.contains_key(path) {
            return Err(GenerateError::ExternalWorkflow(
                ExternalWorkflowError::Collision(path.clone()),
            ));
        }
        let Some(bytes) = on_disk.get(path) else {
            return Err(GenerateError::ExternalWorkflow(
                ExternalWorkflowError::Missing(path.clone()),
            ));
        };
        if owner_of(bytes) == Owner::VelnorGen {
            return Err(GenerateError::ExternalWorkflow(
                ExternalWorkflowError::Generated(path.clone()),
            ));
        }
    }
    Ok(paths.into_iter().collect())
}

/// The skipped set of a repository, computed from its expected files. `plan_generate` checks
/// the same list against the plan it already has; this entry point serves callers that have
/// only the view and the files on disk.
pub fn external_skips(
    view: &RepoView,
    on_disk: &BTreeMap<RepoPath, Vec<u8>>,
) -> Result<BTreeSet<RepoPath>, GenerateError> {
    let expected = crate::expected_files(view)?;
    skipped(view, &expected, on_disk)
}

/// The retired `.velnor/config.toml` still hides every unowned workflow from `check`. Drop
/// those `Unmarked` drifts, and say for each that `external_workflows` replaces the bypass.
pub(crate) fn legacy_bypass(
    drifts: Vec<Drift>,
    engine_configured: bool,
    out: &mut dyn Write,
) -> Vec<Drift> {
    if !engine_configured {
        return drifts;
    }
    drifts
        .into_iter()
        .filter(|drift| {
            let Drift::Unmarked(path) = drift else {
                return true;
            };
            writeln!(out, "velnor-gen: {}", legacy_bypass_notice(path)).ok();
            false
        })
        .collect()
}

/// The deprecation notice for one workflow that only the legacy bypass keeps out of drift.
fn legacy_bypass_notice(path: &RepoPath) -> String {
    format!(
        "notice: {path} is skipped only because .velnor/config.toml exists; that bypass is deprecated, use external_workflows (velnor-ci ≥ 0.1.108)"
    )
}
