//! Credential-free Homebrew updater execution from captured tap main.

use crate::OrchestratorError;
use std::{fs, path::Path, time::Duration};
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes,
    config::{HomebrewTapConfig, MiseCheck, RustBinaryPreviewConfig},
    homebrew_formula::{
        HomebrewFormulaCandidate, VerifiedPreviewRelease, validate_homebrew_formula_candidate,
    },
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
#[path = "tap_prepare_updater.rs"]
mod updater;
#[path = "tap_writer.rs"]
mod writer;
pub(crate) use candidate::reconstruct_historical_candidate_proof;
pub use execute::{PREPARE_HOMEBREW_TAP_OP, prepare_homebrew_tap_from_env};
pub use proof::{HomebrewTapPreparationProof, TapPreparationDisposition};
pub(super) use snapshot::{
    read_formula, sha256, snapshot_tree, validate_formula_only_delta, validate_tree_snapshot,
};
pub use writer::{
    TapPromotionOutcome, TapPromotionReceipt, WRITE_HOMEBREW_TAP_PR_OP,
    write_homebrew_tap_pr_from_env,
};

pub(crate) fn writer_policy_from_env() -> Result<HomebrewTapConfig, OrchestratorError> {
    execute::static_tap_policy()
}
#[cfg(test)]
#[path = "tap_prepare_tests.rs"]
mod tests;

const FORMULA_LIMIT: u64 = 1024 * 1024;
const TREE_LIMIT: usize = 50_000;
const SNAPSHOT_BYTES_LIMIT: u64 = 512 * 1024 * 1024;
const SNAPSHOT_FILE_LIMIT: u64 = 128 * 1024 * 1024;
const TASK_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

/// Execute the trusted tap-main updater without write credentials.
///
/// `release` must come from the upstream authenticated signed-receipt
/// handoff. This function independently revalidates receipt semantics and the
/// formula bytes; signature and attestation verification belong to the
/// caller's release-verification boundary.
///
/// # Errors
/// Rejects an untrusted release, tap config mismatch, task failure, any
/// non-formula tree change, or a formula diff outside the literal contract.
pub(crate) fn prepare_homebrew_tap(
    source_repository: &str,
    tap: &HomebrewTapConfig,
    preview: &RustBinaryPreviewConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPreviewRelease,
    temp: &Path,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    tap.validate(".velnor/config.toml")?;
    preview.validate(".velnor/config.toml")?;
    if !tap.enabled || !preview.enabled || !temp.is_absolute() || !temp.is_dir() {
        return Err(internal("tap_prepare_policy_or_temp"));
    }
    plan.validate()
        .map_err(|_| internal("tap_prepare_plan_invalid"))?;
    release
        .receipt
        .validate_against_preview(preview)
        .map_err(|_| internal("tap_prepare_preview_inventory_mismatch"))?;
    release
        .validate(source_repository, plan)
        .map_err(|_| internal("tap_prepare_receipt_invalid"))?;
    let deadline = CheckDeadline::after(Duration::from_secs(
        u64::from(tap.updater.timeout_minutes) * 60,
    ))
    .map_err(|error| internal(&error.to_string()))?;
    let checkout_path = temp.join("tap-main");
    if fs::symlink_metadata(&checkout_path).is_ok() {
        return Err(internal("tap_checkout_destination_occupied"));
    }
    fs::create_dir(&checkout_path).map_err(|_| internal("tap_checkout_destination_create"))?;
    let result = (|| {
        let captured =
            velnor_actions_mise::capture_tap_main(&tap.repository, &checkout_path, deadline)
                .map_err(|error| internal(&error.to_string()))?;
        prepare_captured_tap(
            source_repository,
            tap,
            release,
            plan,
            temp,
            &captured,
            deadline,
        )
    })();
    if let Err(error) = fs::remove_dir_all(&checkout_path) {
        eprintln!("velnor_tap_checkout_cleanup_failed:{:?}", error.kind());
    }
    result
}

fn prepare_captured_tap(
    source_repository: &str,
    tap: &HomebrewTapConfig,
    release: &VerifiedPreviewRelease,
    plan: &PackageReleasePlan,
    temp: &Path,
    captured: &CapturedTapMain,
    deadline: CheckDeadline,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let tap_config = crate::config::load_config(&captured.checkout)?;
    let base = capture_tap_base(tap, &tap_config, captured, deadline)?;
    let baseline = base.snapshot;
    let base_formula = base.formula;
    let discovered = base.discovered;
    let base_formula_sha = sha256(&base_formula);
    let receipt_bytes =
        canonical_json_bytes(&release.receipt).map_err(|_| internal("tap_receipt_encode"))?;
    if release
        .receipt
        .canonical_digest()
        .map_err(|_| internal("tap_receipt_digest"))?
        != release.receipt_digest
    {
        return Err(internal("tap_receipt_digest_mismatch"));
    }
    let handoff = updater::prepare_receipt_handoff(temp, &receipt_bytes)?;
    if let Some(proof) = current_candidate_proof(&CurrentCandidate {
        source_repository,
        tap,
        release,
        plan,
        temp,
        base_sha: &captured.commit_sha,
        formula: &base_formula,
        formula_sha: &base_formula_sha,
    })? {
        return Ok(proof);
    }
    updater::run_updater(&updater::UpdaterRun {
        root: &captured.checkout,
        temp,
        discovered: &discovered,
        deadline,
        receipt_path: &handoff,
        base_sha: &captured.commit_sha,
        formula: &tap.formula,
        release,
    })?;
    let handed_off = fs::read(&handoff).map_err(|_| internal("tap_receipt_handoff_read"))?;
    if handed_off != receipt_bytes {
        return Err(internal("tap_receipt_handoff_changed"));
    }
    let proposed = read_formula(&captured.checkout, &tap.formula)?;
    let after = snapshot_tree(&captured.checkout)?;
    validate_formula_only_delta(&baseline, &after, &tap.formula, &proposed)?;
    let canonical = validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository,
        tap_repository: &tap.repository,
        formula_path: &tap.formula,
        captured_base_sha: &captured.commit_sha,
        base_bytes: &base_formula,
        proposed_bytes: &proposed,
        plan,
        release,
    })
    .map_err(|_| internal("tap_formula_patch_rejected"))?;
    if sha256(&canonical) == base_formula_sha {
        return Err(internal("tap_formula_candidate_is_unchanged"));
    }
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
            disposition: TapPreparationDisposition::Changed,
        },
    )
}

fn is_exact_current_formula(
    source_repository: &str,
    tap: &HomebrewTapConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPreviewRelease,
    base_sha: &str,
    formula: &[u8],
) -> bool {
    validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository,
        tap_repository: &tap.repository,
        formula_path: &tap.formula,
        captured_base_sha: base_sha,
        base_bytes: formula,
        proposed_bytes: formula,
        plan,
        release,
    })
    .is_ok_and(|canonical| canonical == formula)
}

struct CurrentCandidate<'a> {
    source_repository: &'a str,
    tap: &'a HomebrewTapConfig,
    release: &'a VerifiedPreviewRelease,
    plan: &'a PackageReleasePlan,
    temp: &'a Path,
    base_sha: &'a str,
    formula: &'a [u8],
    formula_sha: &'a str,
}

fn current_candidate_proof(
    candidate: &CurrentCandidate<'_>,
) -> Result<Option<HomebrewTapPreparationProof>, OrchestratorError> {
    if !is_exact_current_formula(
        candidate.source_repository,
        candidate.tap,
        candidate.plan,
        candidate.release,
        candidate.base_sha,
        candidate.formula,
    ) {
        return Ok(None);
    }
    candidate::persist_candidate(
        candidate.temp,
        candidate::CandidateIdentity {
            source_repository: candidate.source_repository,
            tap: candidate.tap,
            release: candidate.release,
            base_sha: candidate.base_sha,
            old_sha: candidate.formula_sha,
            formula: candidate.formula,
            plan: candidate.plan,
            disposition: TapPreparationDisposition::FormulaAlreadyCurrent,
        },
    )
    .map(Some)
}

struct CapturedTapBase {
    discovered: velnor_actions_mise::DiscoveredCheck,
    snapshot: snapshot::Snapshot,
    formula: Vec<u8>,
}

fn capture_tap_base(
    tap: &HomebrewTapConfig,
    config: &velnor_actions_contract::VelnorConfig,
    captured: &CapturedTapMain,
    deadline: CheckDeadline,
) -> Result<CapturedTapBase, OrchestratorError> {
    let check = updater_check(&config.checks, &tap.updater)?;
    let mut discovered = velnor_actions_mise::discover_checks_until(
        &captured.checkout,
        std::slice::from_ref(check),
        &config.qualified_tools,
        deadline,
    )
    .map_err(|error| internal(&error.to_string()))?;
    let discovered = discovered
        .pop()
        .ok_or_else(|| internal("tap_updater_not_discovered"))?;
    if !discovered
        .config_inputs
        .iter()
        .any(|input| input == &tap.updater.source.mise_config)
    {
        return Err(internal("tap_updater_source_not_discovered"));
    }
    let tree =
        velnor_actions_mise::read_tap_tree(&captured.checkout, &captured.commit_sha, deadline)
            .map_err(|error| internal(&error.to_string()))?;
    if tree.len() > TREE_LIMIT {
        return Err(internal("tap_tree_too_large"));
    }
    let snapshot = snapshot_tree(&captured.checkout)?;
    validate_tree_snapshot(&tree, &snapshot)?;
    if snapshot
        .get(&tap.formula)
        .is_none_or(|entry| entry.directory || entry.mode != 0o644)
    {
        return Err(internal("tap_formula_base_mode_invalid"));
    }
    let formula = read_formula(&captured.checkout, &tap.formula)?;
    Ok(CapturedTapBase {
        discovered,
        snapshot,
        formula,
    })
}

fn updater_check<'a>(
    checks: &'a [MiseCheck],
    binding: &velnor_actions_contract::config::TapUpdaterBinding,
) -> Result<&'a MiseCheck, OrchestratorError> {
    let mut matches = checks.iter().filter(|check| {
        check.task == binding.task && check.directory == binding.source.working_directory
    });
    let check = matches
        .next()
        .ok_or_else(|| internal("tap_updater_not_declared_in_tap_main"))?;
    if matches.next().is_some()
        || check.runner != binding.runner
        || check.inputs != binding.inputs
        || check.tools != binding.tools
        || check.timeout_minutes != binding.timeout_minutes
        || !check.system_tools.is_empty()
        || check.evidence.is_some()
    {
        return Err(internal("tap_updater_binding_mismatch"));
    }
    Ok(check)
}

fn internal(problem: &str) -> OrchestratorError {
    crate::internal::internal(problem)
}
