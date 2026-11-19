// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, HelperVersion, MbxStore, PrCi, PushAfterMergeGroup, RepoFacts};

use crate::{BUDGET_HELPER, COMPATIBLE_HELPER_SERIES, GenerateError, HELPER_MINIMUM};

pub(crate) fn check_helper(facts: &RepoFacts) -> Result<&HelperRelease, GenerateError> {
    let release = facts
        .helper
        .as_ref()
        .ok_or(GenerateError::HelperNotPinned)?;
    require_compatible(release.version())?;
    if !release.version().at_least(HELPER_MINIMUM) {
        return Err(GenerateError::HelperTooOld(release.version().clone()));
    }
    if !facts.config.test_groups.is_empty() && !release.version().at_least((0, 1, 82)) {
        return Err(GenerateError::TestGroupsHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.pr_ci == PrCi::Label && !release.version().at_least((0, 1, 86)) {
        return Err(GenerateError::PrCiHelperTooOld(release.version().clone()));
    }
    if facts.config.crates_mirror.is_some() && !release.version().at_least((0, 1, 93)) {
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
    if facts
        .config
        .test_lanes
        .iter()
        .any(|lane| lane.filter.is_some())
        && !release.version().at_least((0, 1, 86))
    {
        return Err(GenerateError::LaneGuardHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.mbx_store == MbxStore::Shared && !release.version().at_least((0, 1, 85)) {
        return Err(GenerateError::MbxStoreHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.budget.is_some_and(|budget| budget.enabled())
        && !release.version().at_least(BUDGET_HELPER)
    {
        return Err(GenerateError::BudgetHelperTooOld(release.version().clone()));
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
