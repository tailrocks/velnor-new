use std::{fs, path::Path};

use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPackageRelease,
    config::{HomebrewTapConfig, MiseCheck},
    homebrew_authority::{
        HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH, HOMEBREW_SOURCE_CONFIG_PATH,
        HOMEBREW_SOURCE_RELEASE_POLICY_PATH, HomebrewSourceAuthorityLocator,
        MAX_HOMEBREW_SOURCE_AUTHORITY_LOCATOR_BYTES, parse_homebrew_source_authority_locator,
    },
    homebrew_formula::{HomebrewFormulaCandidate, validate_homebrew_formula_candidate},
    homebrew_spec::HomebrewFormulaSpec,
};
use velnor_actions_mise::{CapturedTapMain, CheckDeadline, DiscoveredCheck};

use super::{
    CapturedTapPreparation, HomebrewTapPreparationProof, OrchestratorError,
    TapPreparationDisposition, candidate, internal, sha256, snapshot, updater,
};

struct CapturedTapBase {
    tree: Vec<velnor_actions_mise::TapTreeEntry>,
    snapshot: snapshot::Snapshot,
    formula: Vec<u8>,
    discovered: Option<DiscoveredCheck>,
}

pub(super) fn prepare_captured_tap(
    preparation: &CapturedTapPreparation<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let captured = preparation.captured;
    let discovered = match preparation.tap.updater.as_ref() {
        Some(binding) => Some(discover_updater(
            &captured.checkout,
            binding,
            preparation.deadline,
        )?),
        None => None,
    };
    let tree = velnor_actions_mise::read_tap_tree(
        &captured.checkout,
        &captured.commit_sha,
        preparation.deadline,
    )
    .map_err(|error| internal(&error.to_string()))?;
    if tree.len() > super::TREE_LIMIT {
        return Err(internal("tap_tree_too_large"));
    }
    let tree_snapshot = snapshot::snapshot_tree(&captured.checkout)?;
    snapshot::validate_tree_snapshot(&tree, &tree_snapshot)?;
    if !is_sha256(preparation.sdk_artifact_digest) {
        return Err(internal("tap_sdk_artifact_digest_invalid"));
    }
    let base = capture_tap_base(
        captured,
        preparation.formula_path,
        tree,
        tree_snapshot,
        discovered,
    )?;
    match preparation.spec {
        Some(spec) => prepare_native(preparation, &base, spec),
        None => prepare_custom(preparation, &base),
    }
}

fn prepare_native(
    preparation: &CapturedTapPreparation<'_>,
    base: &CapturedTapBase,
    spec: &HomebrewFormulaSpec,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    read_captured_source_locator(
        &preparation.captured.checkout,
        &base.tree,
        &base.snapshot,
        preparation.source_repository,
        preparation.release.receipt.repository.id,
        &preparation.plan.default_branch,
    )?;
    if spec
        .digest()
        .map_err(|_| internal("tap_formula_spec_digest"))?
        .is_empty()
    {
        return Err(internal("tap_formula_spec_binding_mismatch"));
    }
    let canonical = candidate::derive_native_formula_candidate(super::FormulaCandidateInput {
        spec: Some(spec),
        source_repository: preparation.source_repository,
        tap_repository: &preparation.tap.repository,
        formula_path: preparation.formula_path,
        base_sha: &preparation.captured.commit_sha,
        base_formula: &base.formula,
        proposed_formula: None,
        plan: preparation.plan,
        release: preparation.release,
    })?;
    let disposition = if canonical == base.formula {
        TapPreparationDisposition::FormulaAlreadyCurrent
    } else {
        TapPreparationDisposition::Changed
    };
    persist_prepared_candidate(preparation, base, &canonical, disposition, Some(spec))
}

fn prepare_custom(
    preparation: &CapturedTapPreparation<'_>,
    base: &CapturedTapBase,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    if is_exact_current_formula(
        preparation.source_repository,
        preparation.tap,
        preparation.plan,
        preparation.release,
        &preparation.captured.commit_sha,
        &base.formula,
        preparation.formula_path,
    ) {
        return persist_prepared_candidate(
            preparation,
            base,
            &base.formula,
            TapPreparationDisposition::FormulaAlreadyCurrent,
            None,
        );
    }
    let discovered = base
        .discovered
        .as_ref()
        .ok_or_else(|| internal("tap_updater_not_discovered"))?;
    updater::run_captured_updater(&updater::CapturedUpdaterRun {
        release: preparation.release,
        plan: preparation.plan,
        formula_path: preparation.formula_path,
        temp: preparation.temp,
        captured: preparation.captured,
        discovered,
        deadline: preparation.deadline,
    })?;
    let proposed =
        snapshot::read_formula(&preparation.captured.checkout, preparation.formula_path)?;
    let after = snapshot::snapshot_tree(&preparation.captured.checkout)?;
    snapshot::validate_formula_only_delta(
        &base.snapshot,
        &after,
        preparation.formula_path,
        &proposed,
    )?;
    let canonical = validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository: preparation.source_repository,
        tap_repository: &preparation.tap.repository,
        formula_path: preparation.formula_path,
        captured_base_sha: &preparation.captured.commit_sha,
        base_bytes: &base.formula,
        proposed_bytes: &proposed,
        plan: preparation.plan,
        release: preparation.release,
    })
    .map_err(|_| internal("tap_formula_patch_rejected"))?;
    if canonical == base.formula {
        return Err(internal("tap_formula_candidate_is_unchanged"));
    }
    persist_prepared_candidate(
        preparation,
        base,
        &canonical,
        TapPreparationDisposition::Changed,
        None,
    )
}

fn persist_prepared_candidate(
    preparation: &CapturedTapPreparation<'_>,
    base: &CapturedTapBase,
    formula: &[u8],
    disposition: TapPreparationDisposition,
    spec: Option<&HomebrewFormulaSpec>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    if disposition == TapPreparationDisposition::FormulaAlreadyCurrent
        && (snapshot::snapshot_tree(&preparation.captured.checkout)? != base.snapshot
            || snapshot::read_formula(&preparation.captured.checkout, preparation.formula_path)?
                != base.formula)
    {
        return Err(internal("tap_capture_changed_during_preparation"));
    }
    let old_sha = sha256(&base.formula);
    candidate::persist_candidate(
        preparation.temp,
        candidate::CandidateIdentity {
            source_repository: preparation.source_repository,
            tap: preparation.tap,
            release: preparation.release,
            base_sha: &preparation.captured.commit_sha,
            old_sha: &old_sha,
            formula,
            plan: preparation.plan,
            spec,
            sdk_artifact_digest: preparation.sdk_artifact_digest,
            formula_path: preparation.formula_path,
            disposition,
        },
    )
}

fn capture_tap_base(
    captured: &CapturedTapMain,
    formula_path: &str,
    tree: Vec<velnor_actions_mise::TapTreeEntry>,
    snapshot: snapshot::Snapshot,
    discovered: Option<DiscoveredCheck>,
) -> Result<CapturedTapBase, OrchestratorError> {
    if snapshot
        .get(formula_path)
        .is_none_or(|entry| entry.directory || entry.mode != 0o644)
    {
        return Err(internal("tap_formula_base_mode_invalid"));
    }
    let formula = snapshot::read_formula(&captured.checkout, formula_path)?;
    Ok(CapturedTapBase {
        tree,
        snapshot,
        formula,
        discovered,
    })
}

fn is_exact_current_formula(
    source_repository: &str,
    tap: &HomebrewTapConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPackageRelease,
    base_sha: &str,
    formula: &[u8],
    formula_path: &str,
) -> bool {
    validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository,
        tap_repository: &tap.repository,
        formula_path,
        captured_base_sha: base_sha,
        base_bytes: formula,
        proposed_bytes: formula,
        plan,
        release,
    })
    .is_ok_and(|canonical| canonical == formula)
}

pub(super) fn read_captured_source_locator(
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

fn discover_updater(
    checkout: &Path,
    binding: &velnor_actions_contract::config::TapUpdaterBinding,
    deadline: CheckDeadline,
) -> Result<DiscoveredCheck, OrchestratorError> {
    let config = crate::config::load_config(checkout)?;
    let check = updater_check(&config.checks, binding)?;
    let mut discovered = velnor_actions_mise::discover_checks_until(
        checkout,
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
        .any(|input| input == &binding.source.mise_config)
    {
        return Err(internal("tap_updater_source_not_discovered"));
    }
    Ok(discovered)
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

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
