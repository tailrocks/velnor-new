// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde::Serialize;
use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;

#[path = "homebrew_adoption_authoring_cutover.rs"]
mod cutover;
#[path = "homebrew_adoption_authoring_output.rs"]
mod output;
#[path = "homebrew_adoption_authoring_seed.rs"]
mod seed;

const AUTHORING_BUDGET_SECONDS: u64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HomebrewAdoptionAuthoringReport {
    pub output_dir: String,
    pub base_sha: String,
    pub written_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
}

pub fn author_homebrew_adoption_seed(
    transition_id: &str,
    sdk_manifest: &Path,
    output_dir: &Path,
) -> Result<HomebrewAdoptionAuthoringReport, OrchestratorError> {
    let deadline = authoring_deadline()?;
    seed::author(transition_id, sdk_manifest, output_dir, deadline)
}

pub fn author_homebrew_adoption_cutover(
    transition_id: &str,
    sdk_manifest: &Path,
    output_dir: &Path,
) -> Result<HomebrewAdoptionAuthoringReport, OrchestratorError> {
    let deadline = authoring_deadline()?;
    cutover::author(transition_id, sdk_manifest, output_dir, deadline)
}

fn authoring_deadline() -> Result<CheckDeadline, OrchestratorError> {
    CheckDeadline::after(std::time::Duration::from_secs(AUTHORING_BUDGET_SECONDS))
        .map_err(|_| crate::internal::internal("homebrew_adoption_authoring_deadline"))
}
