// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUDGET_MODES_HELPER, CacheDebug, DocsRun, HelperRelease, HelperVersion, MbxStore, Platform,
    PrCi, PushAfterMergeGroup, RepoFacts,
};

use velnor_rust::INPUTS_IDENTITY_HELPER;

use crate::{
    BUDGET_HELPER, COMPATIBLE_HELPER_SERIES, GenerateError, HELPER_MINIMUM, TEST_LANE_HELPER,
    TEST_LANE_JOB_HELPER,
};

/// The first `velnor-ci` whose `.velnor/rust-ci.toml` parser accepts the `hosted` key.
const HOSTED_HELPER: (u64, u64, u64) = (0, 1, 99);
const MERGE_GROUP_HELPER: (u64, u64, u64) = (0, 1, 103);
/// The first `velnor-ci` whose `.velnor/rust-ci.toml` parser accepts the `cache_debug` key.
// helper-gate: first in rust-ci-v0.1.82 via bfc3528bd7
pub(crate) const CACHE_DEBUG_HELPER: (u64, u64, u64) = (0, 1, 82);
/// The first `velnor-ci` that names the `required_check` check when it reads a queued run.
// helper-gate: first in tools-v0.1.97 via af11477df1
pub(crate) const REQUIRED_CHECK_HELPER: (u64, u64, u64) = (0, 1, 97);

/// The first `velnor-ci` whose `.velnor/rust-ci.toml` parser accepts `restore_cache` in a `[[test_groups]]` table.
// helper-gate: first in tools-v0.1.108 via be63bc50f6
pub(crate) const RESTORE_CACHE_HELPER: (u64, u64, u64) = (0, 1, 108);

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
    check_lane_job(facts, release)?;
    check_restore_cache(facts, release)?;
    if facts.config.mbx_store == MbxStore::Shared && !release.version().at_least((0, 1, 85)) {
        return Err(GenerateError::MbxStoreHelperTooOld(
            release.version().clone(),
        ));
    }
    if facts.config.cache_debug == CacheDebug::On && !release.version().at_least(CACHE_DEBUG_HELPER)
    {
        return Err(GenerateError::CacheDebugHelperTooOld(
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

fn check_restore_cache(facts: &RepoFacts, release: &HelperRelease) -> Result<(), GenerateError> {
    let written = facts
        .config
        .test_groups
        .iter()
        .any(|group| group.restore_cache.is_some());
    if written && !release.version().at_least(RESTORE_CACHE_HELPER) {
        return Err(GenerateError::RestoreCacheHelperTooOld(
            release.version().clone(),
        ));
    }
    Ok(())
}

fn check_lane_job(facts: &RepoFacts, release: &HelperRelease) -> Result<(), GenerateError> {
    let written = facts.config.test_lanes.iter().any(|lane| lane.job_written);
    if written && !release.version().at_least(TEST_LANE_JOB_HELPER) {
        return Err(GenerateError::TestLaneJobHelperTooOld(
            release.version().clone(),
        ));
    }
    Ok(())
}

pub fn require_compatible(version: &HelperVersion) -> Result<(), GenerateError> {
    if version.series() == Some(COMPATIBLE_HELPER_SERIES) {
        Ok(())
    } else {
        Err(GenerateError::IncompatibleHelper(version.clone()))
    }
}
