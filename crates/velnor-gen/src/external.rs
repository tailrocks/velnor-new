// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RepoPath, RepoView};
use velnor_render::{Owner, owner_of};
use velnor_rust::detect;

use crate::{ExternalWorkflowError, GenerateError};

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
