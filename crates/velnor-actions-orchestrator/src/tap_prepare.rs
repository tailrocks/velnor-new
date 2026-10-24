//! Native Homebrew formula preparation from captured tap main.

use crate::OrchestratorError;
use std::{fs, path::Path};
use velnor_actions_contract::{
    PackageReleasePlan,
    config::HomebrewTapConfig,
    homebrew_authority::{
        HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH, HOMEBREW_SOURCE_CONFIG_PATH,
        HOMEBREW_SOURCE_RELEASE_POLICY_PATH, HomebrewSourceAuthorityLocator,
        MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES, parse_homebrew_source_authority_locator,
    },
    homebrew_formula::VerifiedPackageRelease,
    homebrew_spec::HomebrewFormulaSpec,
};
use velnor_actions_mise::{CapturedTapMain, CheckDeadline};

#[path = "tap_prepare_candidate.rs"]
mod candidate;
#[path = "tap_prepare_execute.rs"]
mod execute;
#[path = "tap_prepare_proof.rs"]
mod proof;
#[path = "tap_prepare_snapshot.rs"]
mod snapshot;
#[path = "tap_writer.rs"]
mod writer;
pub(crate) use candidate::{FormulaCandidateInput, HistoricalCandidateInput};
pub use execute::{
    ADMIT_HOMEBREW_TAP_OP, PREPARE_HOMEBREW_TAP_OP, admit_homebrew_tap_from_env,
    prepare_homebrew_tap_from_env,
};
pub use proof::{HomebrewTapPreparationProof, TapPreparationDisposition};
pub(super) use snapshot::{read_formula, sha256, snapshot_tree, validate_tree_snapshot};
pub use writer::{
    TapPromotionOutcome, TapPromotionReceipt, WRITE_HOMEBREW_TAP_PR_OP,
    write_homebrew_tap_pr_from_env,
};

pub(crate) fn writer_policy_from_env() -> Result<HomebrewTapConfig, OrchestratorError> {
    execute::static_tap_policy()
}

pub(crate) fn formula_policy_from_env() -> Result<HomebrewFormulaSpec, OrchestratorError> {
    execute::static_formula_policy()
}

pub(crate) fn derive_formula_candidate(
    input: FormulaCandidateInput<'_>,
) -> Result<Vec<u8>, OrchestratorError> {
    candidate::derive_native_formula_candidate(input)
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
#[cfg(test)]
#[path = "tap_prepare_tests.rs"]
mod tests;

const FORMULA_LIMIT: u64 = 1024 * 1024;
const TREE_LIMIT: usize = 50_000;
const SNAPSHOT_BYTES_LIMIT: u64 = 512 * 1024 * 1024;
const SNAPSHOT_FILE_LIMIT: u64 = 128 * 1024 * 1024;

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Prepare canonical formula bytes from authenticated release receipt data.
///
/// # Errors
/// Rejects an untrusted release, tap config mismatch, invalid template, or
/// any captured tree that fails regular-file admission.
#[derive(Clone, Copy)]
pub(crate) struct TapPreparationInput<'a> {
    pub(crate) source_repository: &'a str,
    pub(crate) tap: &'a HomebrewTapConfig,
    pub(crate) plan: &'a PackageReleasePlan,
    pub(crate) release: &'a VerifiedPackageRelease,
    pub(crate) spec: &'a HomebrewFormulaSpec,
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
    if !tap.enabled || !temp.is_absolute() || !temp.is_dir() {
        return Err(internal("tap_prepare_policy_or_temp"));
    }
    plan.validate()
        .map_err(|_| internal("tap_prepare_plan_invalid"))?;
    release
        .validate(source_repository, plan)
        .map_err(|_| internal("tap_prepare_receipt_invalid"))?;
    let formula = formula_path(tap, plan.channel)?;
    let checkout_path = temp.join("tap-main");
    if fs::symlink_metadata(&checkout_path).is_ok() {
        return Err(internal("tap_checkout_destination_occupied"));
    }
    fs::create_dir(&checkout_path).map_err(|_| internal("tap_checkout_destination_create"))?;
    let result = (|| {
        let captured =
            velnor_actions_mise::capture_tap_main(&tap.repository, &checkout_path, deadline)
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
            deadline,
        })
    })();
    if let Err(error) = fs::remove_dir_all(&checkout_path) {
        eprintln!("velnor_tap_checkout_cleanup_failed:{:?}", error.kind());
    }
    result
}

struct CapturedTapPreparation<'a> {
    source_repository: &'a str,
    tap: &'a HomebrewTapConfig,
    release: &'a VerifiedPackageRelease,
    plan: &'a PackageReleasePlan,
    spec: &'a HomebrewFormulaSpec,
    sdk_artifact_digest: &'a str,
    formula_path: &'a str,
    temp: &'a Path,
    captured: &'a CapturedTapMain,
    deadline: CheckDeadline,
}

fn prepare_captured_tap(
    preparation: &CapturedTapPreparation<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let CapturedTapPreparation {
        source_repository,
        tap,
        release,
        plan,
        spec,
        sdk_artifact_digest,
        formula_path,
        temp,
        captured,
        deadline,
    } = preparation;
    let tree =
        velnor_actions_mise::read_tap_tree(&captured.checkout, &captured.commit_sha, *deadline)
            .map_err(|error| internal(&error.to_string()))?;
    if tree.len() > TREE_LIMIT {
        return Err(internal("tap_tree_too_large"));
    }
    let snapshot = snapshot_tree(&captured.checkout)?;
    validate_tree_snapshot(&tree, &snapshot)?;
    read_captured_source_locator(
        &captured.checkout,
        &tree,
        &snapshot,
        source_repository,
        release.receipt.repository.id,
        &plan.default_branch,
    )?;
    let formula_spec_digest = spec
        .digest()
        .map_err(|_| internal("tap_formula_spec_digest"))?;
    if formula_spec_digest.is_empty() || !is_sha256(sdk_artifact_digest) {
        return Err(internal("tap_formula_spec_binding_mismatch"));
    }
    let base = capture_tap_base(captured, formula_path, snapshot)?;
    let base_formula_sha = sha256(&base.formula);
    let canonical = candidate::derive_native_formula_candidate(FormulaCandidateInput {
        spec,
        source_repository,
        tap_repository: &tap.repository,
        formula_path,
        base_sha: &captured.commit_sha,
        base_formula: &base.formula,
        plan,
        release,
    })?;
    if snapshot_tree(&captured.checkout)? != base.snapshot
        || read_formula(&captured.checkout, formula_path)? != base.formula
    {
        return Err(internal("tap_capture_changed_during_preparation"));
    }
    let disposition = if canonical == base.formula {
        TapPreparationDisposition::FormulaAlreadyCurrent
    } else {
        TapPreparationDisposition::Changed
    };
    candidate::persist_candidate(
        temp,
        candidate::CandidateIdentity {
            source_repository,
            tap,
            release,
            base_sha: &captured.commit_sha,
            old_sha: &base_formula_sha,
            formula: &canonical,
            plan,
            spec,
            sdk_artifact_digest,
            formula_path,
            disposition,
        },
    )
}

fn read_captured_source_locator(
    checkout: &Path,
    tree: &[velnor_actions_mise::TapTreeEntry],
    snapshot: &snapshot::Snapshot,
    source_repository: &str,
    source_repository_id: u64,
    default_branch: &str,
) -> Result<HomebrewSourceAuthorityLocator, OrchestratorError> {
    let git_entry = tree
        .iter()
        .find(|entry| entry.path == HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
        .filter(|entry| entry.mode == 0o100_644)
        .ok_or_else(|| internal("tap_source_locator_git_entry_invalid"))?;
    let snapshot_entry = snapshot
        .get(HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)
        .filter(|entry| !entry.directory && entry.mode == 0o644)
        .ok_or_else(|| internal("tap_source_locator_checkout_entry_invalid"))?;
    let path = checkout.join(HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH);
    crate::check_evidence::reject_link_components(
        checkout,
        HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH,
    )?;
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| internal("tap_source_locator_checkout_missing"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES as u64
        || metadata.len() != snapshot_entry.size
    {
        return Err(internal("tap_source_locator_checkout_invalid"));
    }
    let bytes = fs::read(&path).map_err(|_| internal("tap_source_locator_checkout_read"))?;
    if sha256(&bytes) != snapshot_entry.sha256
        || crate::release_policy_admission::git_blob_sha1(&bytes)? != git_entry.blob_sha
    {
        return Err(internal("tap_source_locator_blob_mismatch"));
    }
    let locator = parse_homebrew_source_authority_locator(&bytes)
        .map_err(|_| internal("tap_source_locator_invalid"))?;
    if locator.repository != source_repository
        || locator.repository_id != source_repository_id
        || locator.default_branch != default_branch
        || locator.config_path != HOMEBREW_SOURCE_CONFIG_PATH
        || locator.release_policy_path != HOMEBREW_SOURCE_RELEASE_POLICY_PATH
    {
        return Err(internal("tap_source_locator_source_mismatch"));
    }
    Ok(locator)
}
struct CapturedTapBase {
    snapshot: snapshot::Snapshot,
    formula: Vec<u8>,
}

fn capture_tap_base(
    captured: &CapturedTapMain,
    formula_path: &str,
    snapshot: snapshot::Snapshot,
) -> Result<CapturedTapBase, OrchestratorError> {
    if snapshot
        .get(formula_path)
        .is_none_or(|entry| entry.directory || entry.mode != 0o644)
    {
        return Err(internal("tap_formula_base_mode_invalid"));
    }
    let formula = read_formula(&captured.checkout, formula_path)?;
    Ok(CapturedTapBase { snapshot, formula })
}

fn internal(problem: &str) -> OrchestratorError {
    crate::internal::internal(problem)
}
