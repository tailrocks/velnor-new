// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::inspect::inspect_keg;
use super::request::{formula_name, validate_request, write_formula_file};
use super::support::{
    epoch_seconds, evidence_digest, evidence_from_state, invalid, path_exists, run_brew,
    run_brew_text, snapshot, snapshot_from_file, trusted_owned_directory,
};
use super::{
    HOMEBREW_CELLAR, HOMEBREW_PREFIX, HOMEBREW_VERSION, HomebrewInstallEvidence,
    HomebrewInstallRequest, HomebrewInstalledKegCustody, KegState, tap::TapFormula,
};
use crate::{
    OrchestratorError, filesystem_owner::held_file::HeldFile, homebrew::command::HomebrewCommand,
};

pub(crate) fn upgrade_homebrew_candidate(
    request: &HomebrewInstallRequest<'_>,
    prior_request: &HomebrewInstallRequest<'_>,
    prior: &HomebrewInstalledKegCustody,
) -> Result<HomebrewInstallEvidence, OrchestratorError> {
    let context = prepare_upgrade(request, prior_request, prior)?;
    let upgraded = install_and_test_upgrade(request, prior_request, prior, &context)?;
    verify_prior_after_upgrade(request, prior_request, prior, &context, &upgraded)?;
    let mut evidence = evidence_from_state(
        &context.formula,
        request,
        &upgraded.brew_version,
        &upgraded.prefix,
        &upgraded.cellar,
        &upgraded.keg,
        &upgraded.state,
    );
    evidence.verification_sha256 = evidence_digest(&evidence);
    Ok(evidence)
}

struct UpgradeContext {
    formula: String,
    home: PathBuf,
    temp: PathBuf,
    prior_candidate: HeldFile,
    prior_tap_formula: TapFormula,
}

struct UpgradedCandidate {
    tap_formula: TapFormula,
    canonical_formula: HeldFile,
    keg: PathBuf,
    install_started: u64,
    brew_version: String,
    prefix: String,
    cellar: String,
    state: KegState,
}

struct BrewIdentity {
    version: String,
    prefix: String,
    cellar: String,
}

fn prepare_upgrade(
    request: &HomebrewInstallRequest<'_>,
    prior_request: &HomebrewInstallRequest<'_>,
    prior: &HomebrewInstalledKegCustody,
) -> Result<UpgradeContext, OrchestratorError> {
    validate_request(request)?;
    validate_request(prior_request)?;
    request
        .deadline
        .remaining()
        .map_err(|_| invalid("deadline"))?;
    let formula = formula_name(request)?;
    if formula != formula_name(prior_request)?
        || request.paths.staging == prior_request.paths.staging
        || request.literals.version() == prior_request.literals.version()
        || prior.keg
            != Path::new(HOMEBREW_CELLAR)
                .join(&formula)
                .join(prior_request.literals.version())
        || prior.state.recipe_sha256 != prior_request.formula_sha256
        || prior.state.keg_identity != snapshot_from_file(&prior.directory)?
        || prior.state.keg_identity != snapshot(&prior.keg)?
    {
        return Err(invalid("prior_keg_custody_mismatch"));
    }
    let home = trusted_owned_directory(&request.paths.home)?;
    let temp = trusted_owned_directory(&request.paths.temp)?;
    let staging = trusted_owned_directory(&prior_request.paths.staging)?;
    let prior_candidate = HeldFile::new(
        &staging.join(format!("{formula}.rb")),
        prior_request.formula_sha256,
        prior_request.formula_bytes.len() as u64,
        request.deadline,
    )?;
    prior_candidate.verify(request.deadline)?;
    let prior_tap_formula = TapFormula::capture_existing(
        &formula,
        prior_request.formula_bytes,
        prior_request.formula_sha256,
        request.deadline,
    )?;
    let prior_before = inspect_keg(
        prior_request,
        &prior.keg,
        &prior_tap_formula.path,
        prior.state.install_time,
    )?;
    if prior_before != prior.state {
        return Err(invalid("prior_keg_changed_before_upgrade"));
    }
    Ok(UpgradeContext {
        formula,
        home,
        temp,
        prior_candidate,
        prior_tap_formula,
    })
}

fn install_and_test_upgrade(
    request: &HomebrewInstallRequest<'_>,
    prior_request: &HomebrewInstallRequest<'_>,
    prior: &HomebrewInstalledKegCustody,
    context: &UpgradeContext,
) -> Result<UpgradedCandidate, OrchestratorError> {
    let candidate_formula = stage_upgrade_formula(request, &context.formula)?;
    let identity = verified_brew_identity(&context.home, &context.temp, request.deadline)?;
    let new_keg = Path::new(HOMEBREW_CELLAR)
        .join(&context.formula)
        .join(request.literals.version());
    if new_keg == prior.keg || path_exists(&new_keg)? {
        return Err(invalid("candidate_keg_already_exists"));
    }
    let tap_formula = context.prior_tap_formula.replace(
        request.formula_bytes,
        request.formula_sha256,
        prior_request.formula_bytes,
        request.deadline,
    )?;
    run_brew(
        HomebrewCommand::TrustFormula {
            formula: super::tap::formula_reference(&context.formula)?,
        },
        &context.home,
        &context.temp,
        request.deadline,
    )?;
    tap_formula.verify(request.deadline)?;
    let install_started = epoch_seconds()?;
    run_brew(
        HomebrewCommand::UpgradeCandidate {
            recipe: tap_formula.path.clone(),
        },
        &context.home,
        &context.temp,
        request.deadline,
    )?;
    candidate_formula.verify(request.deadline)?;
    let after = inspect_keg(request, &new_keg, &tap_formula.path, install_started)?;
    let d_recipe = new_keg
        .join(".brew")
        .join(format!("{}.rb", context.formula));
    let d_archived = HeldFile::new(
        &d_recipe,
        request.formula_sha256,
        request.formula_bytes.len() as u64,
        request.deadline,
    )?;
    run_brew(
        HomebrewCommand::TestArchived { recipe: d_recipe },
        &context.home,
        &context.temp,
        request.deadline,
    )?;
    candidate_formula.verify(request.deadline)?;
    tap_formula.verify(request.deadline)?;
    d_archived.verify(request.deadline)?;
    let d_after_test = inspect_keg(request, &new_keg, &tap_formula.path, install_started)?;
    if after != d_after_test {
        return Err(invalid("upgraded_keg_changed_during_test"));
    }
    Ok(UpgradedCandidate {
        tap_formula,
        canonical_formula: candidate_formula,
        keg: new_keg,
        install_started,
        brew_version: identity.version,
        prefix: identity.prefix,
        cellar: identity.cellar,
        state: d_after_test,
    })
}

fn stage_upgrade_formula(
    request: &HomebrewInstallRequest<'_>,
    formula: &str,
) -> Result<HeldFile, OrchestratorError> {
    let formula_path = request.paths.staging.join(format!("{formula}.rb"));
    let staging = trusted_owned_directory(&request.paths.staging)?;
    if !formula_path.starts_with(&staging) {
        return Err(invalid("formula_outside_staging"));
    }
    write_formula_file(
        &formula_path,
        request.formula_bytes,
        request.formula_sha256,
        request.deadline,
    )?;
    HeldFile::new(
        &formula_path,
        request.formula_sha256,
        request.formula_bytes.len() as u64,
        request.deadline,
    )
}

fn verified_brew_identity(
    home: &Path,
    temp: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<BrewIdentity, OrchestratorError> {
    let version = run_brew_text(HomebrewCommand::Version, home, temp, deadline)?;
    if version.lines().next() != Some(HOMEBREW_VERSION) {
        return Err(invalid("unqualified_homebrew_version"));
    }
    let prefix = run_brew_text(HomebrewCommand::Prefix, home, temp, deadline)?;
    if Path::new(prefix.trim()) != Path::new(HOMEBREW_PREFIX) {
        return Err(invalid("unqualified_homebrew_prefix"));
    }
    let cellar = run_brew_text(HomebrewCommand::Cellar, home, temp, deadline)?;
    if Path::new(cellar.trim()) != Path::new(HOMEBREW_CELLAR) {
        return Err(invalid("unqualified_homebrew_cellar"));
    }
    Ok(BrewIdentity {
        version,
        prefix,
        cellar,
    })
}

fn verify_prior_after_upgrade(
    request: &HomebrewInstallRequest<'_>,
    prior_request: &HomebrewInstallRequest<'_>,
    prior: &HomebrewInstalledKegCustody,
    context: &UpgradeContext,
    upgraded: &UpgradedCandidate,
) -> Result<(), OrchestratorError> {
    let a_recipe = prior
        .keg
        .join(".brew")
        .join(format!("{}.rb", context.formula));
    let a_archived = HeldFile::new(
        &a_recipe,
        prior_request.formula_sha256,
        prior_request.formula_bytes.len() as u64,
        request.deadline,
    )?;
    run_brew(
        HomebrewCommand::TestArchived { recipe: a_recipe },
        &context.home,
        &context.temp,
        request.deadline,
    )?;
    context.prior_candidate.verify(request.deadline)?;
    a_archived.verify(request.deadline)?;
    let prior_after = inspect_keg(
        prior_request,
        &prior.keg,
        &context.prior_tap_formula.path,
        prior.state.install_time,
    )?;
    if prior_after != prior.state
        || snapshot_from_file(&prior.directory)? != prior.state.keg_identity
        || snapshot(&prior.keg)? != prior.state.keg_identity
    {
        return Err(invalid("prior_keg_changed_during_upgrade"));
    }
    let d_after_prior_test = inspect_keg(
        request,
        &upgraded.keg,
        &upgraded.tap_formula.path,
        upgraded.install_started,
    )?;
    if d_after_prior_test != upgraded.state {
        return Err(invalid("upgraded_keg_changed_during_prior_test"));
    }
    upgraded.tap_formula.verify(request.deadline)?;
    upgraded.canonical_formula.verify(request.deadline)?;
    context.prior_candidate.verify(request.deadline)?;
    Ok(())
}
