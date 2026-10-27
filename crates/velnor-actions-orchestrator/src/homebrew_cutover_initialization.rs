// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::github::TapMergeGitHub;
use crate::homebrew::structural_cutover::{MARKER_LIMIT, MARKER_PATH, parse_marker, sha256};
use crate::tap_writer_history_timestamp as timestamp;
use crate::{OrchestratorError, internal::internal, tap_prepare::TapPromotionOutcome};
use std::time::{Duration, Instant};
use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPackageRelease, config::HomebrewTapConfig,
    homebrew_spec::HomebrewFormulaSpec, package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

#[path = "homebrew_cutover_checkout.rs"]
mod checkout;
#[path = "homebrew_cutover_history.rs"]
mod history;
#[path = "homebrew_cutover_replay.rs"]
mod replay;

pub(crate) struct CutoverInput<'a> {
    pub(crate) policy: &'a HomebrewTapConfig,
    pub(crate) spec: Option<&'a HomebrewFormulaSpec>,
    pub(crate) plan: &'a PackageReleasePlan,
    pub(crate) release: &'a VerifiedPackageRelease,
    pub(crate) formula: &'a [u8],
    pub(crate) sdk_artifact_digest: &'a str,
}

pub(super) struct ObservedCutoverInitialization {
    pub(super) pull: history::ObservedCutoverPull,
    pub(super) marker_sha256: String,
    pub(super) current_main: String,
}

impl ObservedCutoverInitialization {
    pub(super) fn outcome(&self) -> TapPromotionOutcome {
        TapPromotionOutcome::InitializedByCutover {
            cutover_pull_request: self.pull.number,
            head_sha: self.pull.head.clone(),
            base_sha: self.pull.base.clone(),
            merge_sha: self.pull.landed.clone(),
            merged_at: self.pull.merged_at.clone(),
            marker_sha256: self.marker_sha256.clone(),
        }
    }
}

pub(crate) fn observe_cutover_initialization(
    input: &CutoverInput<'_>,
) -> Result<TapPromotionOutcome, OrchestratorError> {
    let budget = Duration::from_secs(
        60 * u64::from(velnor_actions_contract::workflow::JobTimeout::DEFAULT_MINUTES),
    );
    let deadline = CheckDeadline::after(budget)
        .map_err(|_| internal("homebrew_cutover_initialization_deadline"))?;
    let api = TapMergeGitHub::with_deadline(
        super::required_token("VELNOR_TAP_TOKEN")?,
        Instant::now() + budget,
    );
    observe(input, &api, deadline).map(|observed| observed.outcome())
}

pub(super) fn observe(
    input: &CutoverInput<'_>,
    api: &TapMergeGitHub,
    deadline: CheckDeadline,
) -> Result<ObservedCutoverInitialization, OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    let policy = transition.transition();
    if input.plan.channel != PackageReleaseChannel::Preview
        || input.policy.repository != policy.tap_repository
    {
        return Err(internal("homebrew_cutover_initialization_selection"));
    }
    let tip = crate::package_release_publish::read_public_branch_tip(
        &policy.tap_repository,
        policy.tap_repository_id,
        "main",
        deadline,
    )?;
    let bytes = read_marker(
        &policy.tap_repository,
        policy.tap_repository_id,
        &tip,
        deadline,
    )?;
    let marker = parse_marker(&bytes)?;
    let pull = history::find(api, &policy.tap_repository, policy.tap_repository_id, &tip)?;
    replay::verify(input, &marker, &pull, api, deadline)?;
    verify_current(input, api, &pull, &tip, deadline)?;
    Ok(ObservedCutoverInitialization {
        pull,
        marker_sha256: sha256(&bytes),
        current_main: tip,
    })
}

fn read_marker(
    repository: &str,
    repository_id: u64,
    tip: &str,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let bytes = crate::package_release_publish::read_protected_source_file(
        repository,
        repository_id,
        "main",
        tip,
        MARKER_PATH,
        MARKER_LIMIT,
        deadline,
    )?;
    let tree = crate::package_release_publish::read_protected_source_tree_identity(
        repository,
        repository_id,
        tip,
        deadline,
    )?;
    let blob = crate::release_policy_admission::git_blob_sha1(&bytes)?;
    if !tree
        .regular_files
        .iter()
        .any(|entry| entry.path == MARKER_PATH && entry.mode == "100644" && entry.sha == blob)
    {
        return Err(internal("homebrew_cutover_marker_file_identity"));
    }
    Ok(bytes)
}

fn verify_current(
    input: &CutoverInput<'_>,
    api: &TapMergeGitHub,
    pull: &history::ObservedCutoverPull,
    tip: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let transition = crate::homebrew::structural_cutover::protected_transition()?;
    let policy = transition.transition();
    let current_formula = crate::package_release_publish::read_protected_source_file(
        &policy.tap_repository,
        policy.tap_repository_id,
        "main",
        tip,
        crate::tap_prepare::formula_path(input.policy, PackageReleaseChannel::Preview)?,
        1024 * 1024,
        deadline,
    )?;
    let final_tip = crate::package_release_publish::verify_public_protected_main_lineage(
        &policy.tap_repository,
        policy.tap_repository_id,
        "main",
        &pull.base,
        &pull.landed,
        deadline,
    )?;
    if current_formula != input.formula
        || final_tip != tip
        || history::read(
            api,
            &policy.tap_repository,
            policy.tap_repository_id,
            pull.number,
        )? != *pull
    {
        return Err(internal("homebrew_cutover_initialization_changed"));
    }
    Ok(())
}
