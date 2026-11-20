// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUDGET_MODES_HELPER, DocsRun, HelperRelease, HelperVersion, Platform, PushAfterMergeGroup,
    RepoFacts,
};

use velnor_rust::INPUTS_IDENTITY_HELPER;

use crate::{COMPATIBLE_HELPER_SERIES, GenerateError, HELPER_MINIMUM};

const MERGE_GROUP_HELPER: (u64, u64, u64) = (0, 1, 103);
/// The first `velnor-ci` that names the `required_check` check when it reads a queued run.
// helper-gate: first in tools-v0.1.97 via af11477df1
pub(crate) const REQUIRED_CHECK_HELPER: (u64, u64, u64) = (0, 1, 97);

pub(crate) fn check_helper(facts: &RepoFacts) -> Result<&HelperRelease, GenerateError> {
    let release = facts
        .helper
        .as_ref()
        .ok_or(GenerateError::HelperNotPinned)?;
    require_compatible(release.version())?;
    if !release.version().at_least(HELPER_MINIMUM) {
        return Err(GenerateError::HelperTooOld(release.version().clone()));
    }
    if facts
        .config
        .runner
        .as_str()
        .to_ascii_lowercase()
        .starts_with("macos")
        && !release.digests().contains_key(&Platform::MacosArm64)
    {
        return Err(GenerateError::HelperLockLacksMacos {
            version: release.version().clone(),
            runner: facts.config.runner.to_string(),
        });
    }
    // helper-gate: first in tools-v0.1.97 via e25d599090
    if facts.config.docs_run == DocsRun::PostMerge && !release.version().at_least((0, 1, 97)) {
        return Err(GenerateError::DocsHelperTooOld(release.version().clone()));
    }
    // helper-gate: first in tools-v0.1.97 via e25d599090
    if facts.config.check_alone_run == DocsRun::PostMerge && !release.version().at_least((0, 1, 97))
    {
        return Err(GenerateError::CheckAloneHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.inputs_lock_identified && !release.version().at_least(INPUTS_IDENTITY_HELPER) {
        return Err(GenerateError::InputsLockHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.push_after_merge_group == PushAfterMergeGroup::SkipWhenQueueRunPassed
        && !release.version().at_least(MERGE_GROUP_HELPER)
    {
        return Err(GenerateError::MergeGroupHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.required_check != "Required"
        && !release.version().at_least(REQUIRED_CHECK_HELPER)
    {
        return Err(GenerateError::RequiredCheckHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.budget.is_some_and(|budget| budget.has_modes())
        && !release.version().at_least(BUDGET_MODES_HELPER)
    {
        return Err(GenerateError::BudgetModesHelperTooOld(
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
