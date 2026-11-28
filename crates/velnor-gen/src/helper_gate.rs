// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    BUDGET_MODES_HELPER, DocsRun, HelperRelease, HelperVersion, LintKind, MisePlatform, Platform,
    PushAfterMergeGroup, RELEASE_CONFIG_PATH, RepoFacts, RepoView, WORKFLOWS_CONFIG_PATH,
};

use velnor_rust::{
    INPUTS_IDENTITY_HELPER, SCOPED_LOCKS_HELPER, TOKEN_FIRST_HELPER, check_lint_tools, check_mbx,
    check_tools, complete_profile,
};

use crate::{COMPATIBLE_HELPER_SERIES, GenerateError, HELPER_MINIMUM};

const MERGE_GROUP_HELPER: (u64, u64, u64) = (0, 1, 103);
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
    if is_macos(facts) && !release.digests().contains_key(&Platform::MacosArm64) {
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
    require_generator_digests(facts, release)?;
    Ok(release)
}

fn require_generator_digests(
    facts: &RepoFacts,
    release: &HelperRelease,
) -> Result<(), GenerateError> {
    let wanted = facts
        .config
        .lint
        .iter()
        .any(|lint| matches!(lint.kind, LintKind::GeneratedCheck { .. }));
    if !wanted {
        return Ok(());
    }
    if is_macos_x64(facts) {
        return Err(GenerateError::GeneratorMacosX64 {
            runner: facts.config.runner.to_string(),
        });
    }
    let Some(digests) = release.generator_digests() else {
        return Err(GenerateError::GeneratorNotLocked(release.version().clone()));
    };
    let installs_archive = facts.config.lint.iter().any(|lint| {
        matches!(
            lint.kind,
            LintKind::GeneratedCheck {
                from_source: false,
                ..
            }
        )
    });
    if installs_archive && is_macos(facts) && !digests.contains_key(&Platform::MacosArm64) {
        return Err(GenerateError::GeneratorLockLacksMacos {
            version: release.version().clone(),
            runner: facts.config.runner.to_string(),
        });
    }
    Ok(())
}

fn is_macos(facts: &RepoFacts) -> bool {
    MisePlatform::runner(facts.config.runner.as_str())
        .is_some_and(|platform| platform.matches("macos"))
}

fn is_macos_x64(facts: &RepoFacts) -> bool {
    MisePlatform::runner(facts.config.runner.as_str())
        .is_some_and(|platform| platform.matches("macos/x64"))
}

pub fn require_compatible(version: &HelperVersion) -> Result<(), GenerateError> {
    if version.series() == Some(COMPATIBLE_HELPER_SERIES) {
        Ok(())
    } else {
        Err(GenerateError::IncompatibleHelper(version.clone()))
    }
}

pub(crate) fn pinned_helper<'a>(
    view: &RepoView,
    facts: &'a RepoFacts,
) -> Result<Option<&'a HelperRelease>, GenerateError> {
    if !facts.rust_roots.is_empty() || facts.helper.is_some() || needs_pinned_helper(view, facts) {
        let helper = check_helper(facts)?;
        crate::key_gates::check_config_keys(view, helper.version())?;
        check_complete_profile(facts, helper)?;
        return Ok(Some(helper));
    }
    if crate::key_gates::required_helper(view).is_some() {
        return Err(GenerateError::HelperNotPinned);
    }
    Ok(None)
}

fn check_complete_profile(facts: &RepoFacts, helper: &HelperRelease) -> Result<(), GenerateError> {
    if complete_profile(facts) && !helper.version().at_least(SCOPED_LOCKS_HELPER) {
        return Err(GenerateError::ConfigKeyHelperTooOld {
            file: "rust-toolchain.toml".to_owned(),
            key: "toolchain.profile",
            value: Some("complete"),
            first: SCOPED_LOCKS_HELPER,
            pinned: helper.version().clone(),
        });
    }
    Ok(())
}

pub(crate) fn check_stack_tools(view: &RepoView, facts: &RepoFacts) -> Result<(), GenerateError> {
    if !facts.rust_roots.is_empty() {
        check_tools(facts)?;
        check_mbx(facts, &view.files)?;
    } else if needs_lint_tools(facts) {
        check_lint_tools(facts)?;
    }
    Ok(())
}

fn needs_pinned_helper(view: &RepoView, facts: &RepoFacts) -> bool {
    view.files.contains_key(&WORKFLOWS_CONFIG_PATH)
        || view.files.contains_key(&RELEASE_CONFIG_PATH)
        || facts
            .config
            .lint
            .iter()
            .any(|lint| matches!(lint.kind, LintKind::GeneratedCheck { .. }) || lint.planned())
}

fn needs_lint_tools(facts: &RepoFacts) -> bool {
    facts.config.lint.iter().any(|lint| {
        matches!(
            lint.kind,
            LintKind::Jactionlint { .. } | LintKind::Alint { .. } | LintKind::MiseTask { .. }
        )
    })
}

pub(crate) fn mint_note(facts: &RepoFacts) -> Option<String> {
    let version = facts.helper.as_ref()?.version();
    let uses_token =
        facts.config.private_sources.is_some() || !facts.config.release_assets.is_empty();
    (uses_token && !version.at_least(TOKEN_FIRST_HELPER)).then(|| {
        let (major, minor, patch) = TOKEN_FIRST_HELPER;
        format!(
            "[private_sources] / release assets with helper {} mint the App token after repository-controlled steps; upgrade the helper to {major}.{minor}.{patch} or later",
            version.as_str()
        )
    })
}
