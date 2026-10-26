// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use std::{fs, path::Path, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPackageRelease, config::HomebrewTapConfig,
    homebrew_spec::HomebrewFormulaSpec,
};
use velnor_actions_mise::{CapturedTapMain, CheckDeadline};

#[path = "tap_prepare_candidate.rs"]
mod candidate;
#[path = "tap_prepare_execute.rs"]
mod execute;
#[path = "tap_prepare_proof.rs"]
mod proof;
#[path = "tap_prepare_routes.rs"]
mod routes;
#[path = "tap_prepare_snapshot.rs"]
mod snapshot;
#[path = "tap_prepare_updater.rs"]
mod updater;
#[path = "tap_writer.rs"]
mod writer;
pub(crate) use candidate::{FormulaCandidateInput, HistoricalCandidateInput};
pub use execute::{
    ADMIT_HOMEBREW_TAP_OP, PREPARE_HOMEBREW_TAP_OP, admit_homebrew_tap_from_env,
    prepare_homebrew_tap_from_env,
};
pub use proof::{HomebrewTapPreparationProof, TapPreparationDisposition};
pub(super) use snapshot::sha256;
pub use writer::{
    TapPromotionOutcome, TapPromotionReceipt, WRITE_HOMEBREW_TAP_PR_OP,
    write_homebrew_tap_pr_from_env,
};

pub(crate) fn writer_policy_from_env() -> Result<HomebrewTapConfig, OrchestratorError> {
    execute::static_tap_policy()
}

pub(crate) fn formula_policy_from_env() -> Result<Option<HomebrewFormulaSpec>, OrchestratorError> {
    execute::static_formula_policy()
}

pub(crate) fn derive_formula_candidate(
    input: FormulaCandidateInput<'_>,
) -> Result<Vec<u8>, OrchestratorError> {
    candidate::derive_formula_candidate(input)
}

pub(crate) fn reconstruct_historical_candidate_proof(
    input: HistoricalCandidateInput<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    candidate::reconstruct_historical_candidate_proof(input)
}

pub(crate) fn formula_path(
    tap: &HomebrewTapConfig,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Result<&str, OrchestratorError> {
    tap.destinations
        .get(channel)
        .map(|destination| destination.formula.as_str())
        .ok_or_else(|| internal("tap_channel_destination_missing"))
}

pub(crate) fn release_policy_from_env()
-> Result<velnor_actions_contract::PackageReleasePolicy, OrchestratorError> {
    let raw = std::env::var("VELNOR_RELEASE_POLICY")
        .map_err(|_| internal("tap_release_policy_missing"))?;
    velnor_actions_contract::parse_package_release_policy(raw.as_bytes())
        .map_err(|_| internal("tap_release_policy_invalid"))
}

const FORMULA_LIMIT: u64 = 1024 * 1024;
const TREE_LIMIT: usize = 50_000;
const SNAPSHOT_BYTES_LIMIT: u64 = 512 * 1024 * 1024;
const SNAPSHOT_FILE_LIMIT: u64 = 128 * 1024 * 1024;
const TASK_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct TapPreparationInput<'a> {
    pub(crate) source_repository: &'a str,
    pub(crate) tap: &'a HomebrewTapConfig,
    pub(crate) plan: &'a PackageReleasePlan,
    pub(crate) release: &'a VerifiedPackageRelease,
    pub(crate) spec: Option<&'a HomebrewFormulaSpec>,
    pub(crate) sdk_artifact_digest: &'a str,
    pub(crate) temp: &'a Path,
    pub(crate) deadline: CheckDeadline,
}

pub(crate) fn prepare_homebrew_tap(
    input: TapPreparationInput<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let TapPreparationInput {
        source_repository,
        tap,
        plan,
        release,
        spec,
        sdk_artifact_digest,
        temp,
        deadline,
    } = input;
    tap.validate(".velnor/config.toml")?;
    if !tap.enabled
        || !temp.is_absolute()
        || !temp.is_dir()
        || tap.updater.is_some() == spec.is_some()
    {
        return Err(internal("tap_prepare_policy_or_temp"));
    }
    plan.validate()
        .map_err(|_| internal("tap_prepare_plan_invalid"))?;
    release
        .validate(source_repository, plan)
        .map_err(|_| internal("tap_prepare_receipt_invalid"))?;
    let formula = formula_path(tap, plan.channel)?;
    let operation_deadline = if let Some(updater) = &tap.updater {
        CheckDeadline::after(Duration::from_secs(u64::from(updater.timeout_minutes) * 60))
            .map_err(|error| internal(&error.to_string()))?
    } else {
        deadline
    };
    let checkout_path = temp.join("tap-main");
    if fs::symlink_metadata(&checkout_path).is_ok() {
        return Err(internal("tap_checkout_destination_occupied"));
    }
    fs::create_dir(&checkout_path).map_err(|_| internal("tap_checkout_destination_create"))?;
    let result = scope_operation_deadlines(deadline, operation_deadline, || {
        let captured = velnor_actions_mise::capture_tap_main(
            &tap.repository,
            &checkout_path,
            operation_deadline,
        )
        .map_err(|error| internal(&error.to_string()))?;
        prepare_captured_tap(&CapturedTapPreparation {
            source_repository,
            tap,
            release,
            plan,
            spec,
            sdk_artifact_digest,
            formula_path: formula,
            temp,
            captured: &captured,
            deadline: operation_deadline,
        })
    });
    if let Err(error) = fs::remove_dir_all(&checkout_path) {
        eprintln!("velnor_tap_checkout_cleanup_failed:{:?}", error.kind());
    }
    result
}

pub(super) struct CapturedTapPreparation<'a> {
    source_repository: &'a str,
    tap: &'a HomebrewTapConfig,
    release: &'a VerifiedPackageRelease,
    plan: &'a PackageReleasePlan,
    spec: Option<&'a HomebrewFormulaSpec>,
    sdk_artifact_digest: &'a str,
    formula_path: &'a str,
    temp: &'a Path,
    captured: &'a CapturedTapMain,
    deadline: CheckDeadline,
}

pub(super) fn prepare_captured_tap(
    preparation: &CapturedTapPreparation<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    routes::prepare_captured_tap(preparation)
}

fn internal(problem: &str) -> OrchestratorError {
    crate::internal::internal(problem)
}

fn scope_operation_deadlines<T>(
    caller: CheckDeadline,
    operation: CheckDeadline,
    work: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let nested = caller
        .scope(|| operation.scope(work))
        .map_err(|error| internal(&error.to_string()))?;
    nested.map_err(|error| internal(&error.to_string()))?
}
