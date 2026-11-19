// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUDGET_MODES_HELPER, DocsRun, HelperRelease, HelperVersion, MbxStore, Platform, PrCi,
    PushAfterMergeGroup, RepoFacts,
};

use velnor_rust::INPUTS_IDENTITY_HELPER;

use crate::{
    BUDGET_HELPER, COMPATIBLE_HELPER_SERIES, GenerateError, HELPER_MINIMUM, TEST_LANE_HELPER,
};

/// The first `velnor-ci` whose `.velnor/rust-ci.toml` parser accepts the `hosted` key.
const HOSTED_HELPER: (u64, u64, u64) = (0, 1, 99);
const MERGE_GROUP_HELPER: (u64, u64, u64) = (0, 1, 103);

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
    // helper-gate: first in tools-v0.1.90 via 85f4392434
    if !facts.config.test_groups.is_empty() && !release.version().at_least((0, 1, 90)) {
        return Err(GenerateError::TestGroupsHelperTooOld(
            release.version().clone(),
        ));
    }
    // helper-gate: first in tools-v0.1.88 via e061b56eac
    if facts.config.pr_ci == PrCi::Label && !release.version().at_least((0, 1, 88)) {
        return Err(GenerateError::PrCiHelperTooOld(release.version().clone()));
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
    if facts.config.crates_mirror.is_some() && !release.version().at_least((0, 1, 93)) {
        return Err(GenerateError::MirrorHelperTooOld(release.version().clone()));
    }
    if facts.config.hosted_runners.key_written() && !release.version().at_least(HOSTED_HELPER) {
        return Err(GenerateError::HostedHelperTooOld(release.version().clone()));
    }
    if facts.inputs_lock_identified && !release.version().at_least(INPUTS_IDENTITY_HELPER) {
        return Err(GenerateError::InputsLockHelperTooOld(
            release.version().clone(),
        ));
    }
    // helper-gate: first in tools-v0.1.88 via eb3f4deda4
    if facts.config.private_sources.is_some() && !release.version().at_least((0, 1, 88)) {
        return Err(GenerateError::PrivateSourcesHelperTooOld(
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
    if !facts.config.test_lanes.is_empty() && !release.version().at_least(TEST_LANE_HELPER) {
        return Err(GenerateError::TestLaneHelperTooOld(
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
