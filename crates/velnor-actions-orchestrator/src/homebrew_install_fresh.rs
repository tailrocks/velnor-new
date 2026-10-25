use std::path::{Path, PathBuf};

use super::inspect::inspect_keg;
use super::request::{formula_name, validate_request, write_candidate_formula};
use super::support::{
    epoch_seconds, evidence_digest, evidence_from_state, invalid, open_keg_directory, path_exists,
    run_brew, run_brew_text, snapshot, snapshot_from_file, trusted_owned_directory,
    verify_trusted_path,
};
use super::{
    HOMEBREW_CELLAR, HOMEBREW_PREFIX, HOMEBREW_VERSION, HomebrewInstallEvidence,
    HomebrewInstallRequest, HomebrewInstalledKegCustody, KegState, tap::TapFormula,
};
use crate::{
    OrchestratorError, filesystem_owner::held_file::HeldFile, homebrew::command::HomebrewCommand,
};

pub(crate) fn install_homebrew_candidate(
    request: &HomebrewInstallRequest<'_>,
) -> Result<HomebrewInstallEvidence, OrchestratorError> {
    install_homebrew_candidate_with_custody(request).map(|(evidence, _)| evidence)
}

pub(crate) fn install_homebrew_candidate_with_custody(
    request: &HomebrewInstallRequest<'_>,
) -> Result<(HomebrewInstallEvidence, HomebrewInstalledKegCustody), OrchestratorError> {
    let prepared = prepare_candidate_install(request)?;
    let (keg, state) = install_and_test_fresh_keg(request, &prepared)?;
    finish_fresh_install(request, &prepared, keg, state)
}

struct CandidateInstall {
    formula: String,
    home: PathBuf,
    temp: PathBuf,
    candidate_formula: HeldFile,
    tap_formula: TapFormula,
    brew_version: String,
    prefix: String,
    cellar: String,
}

fn prepare_candidate_install(
    request: &HomebrewInstallRequest<'_>,
) -> Result<CandidateInstall, OrchestratorError> {
    validate_request(request)?;
    request
        .deadline
        .remaining()
        .map_err(|_| invalid("deadline"))?;

    let formula_name = formula_name(request)?;
    let home = trusted_owned_directory(&request.paths.home)?;
    let temp = trusted_owned_directory(&request.paths.temp)?;
    let staging = trusted_owned_directory(&request.paths.staging)?;
    let formula_path = write_candidate_formula(request, &formula_name)?;
    let candidate_formula = HeldFile::new(
        &formula_path,
        request.formula_sha256,
        request.formula_bytes.len() as u64,
        request.deadline,
    )?;
    if !formula_path.starts_with(&staging) {
        return Err(invalid("formula_outside_staging"));
    }
    let brew_version = run_brew_text(HomebrewCommand::Version, &home, &temp, request.deadline)?;
    if brew_version.lines().next() != Some(HOMEBREW_VERSION) {
        return Err(invalid("unqualified_homebrew_version"));
    }
    let prefix = run_brew_text(HomebrewCommand::Prefix, &home, &temp, request.deadline)?;
    if Path::new(prefix.trim()) != Path::new(HOMEBREW_PREFIX) {
        return Err(invalid("unqualified_homebrew_prefix"));
    }
    let cellar = run_brew_text(HomebrewCommand::Cellar, &home, &temp, request.deadline)?;
    if Path::new(cellar.trim()) != Path::new(HOMEBREW_CELLAR) {
        return Err(invalid("unqualified_homebrew_cellar"));
    }
    let prefix_path = Path::new(HOMEBREW_PREFIX);
    let _prefix = trusted_owned_directory(prefix_path)?;
    let _cellar = trusted_owned_directory(Path::new(HOMEBREW_CELLAR))?;

    let rack = Path::new(HOMEBREW_CELLAR).join(&formula_name);
    let opt = prefix_path.join("opt").join(&formula_name);
    if path_exists(&rack)? || path_exists(&opt)? {
        return Err(invalid("formula_already_installed"));
    }
    let tap_formula = TapFormula::create_fresh(
        &formula_name,
        request.formula_bytes,
        request.formula_sha256,
        request.deadline,
    )?;
    Ok(CandidateInstall {
        formula: formula_name,
        home,
        temp,
        candidate_formula,
        tap_formula,
        brew_version,
        prefix,
        cellar,
    })
}

fn install_and_test_fresh_keg(
    request: &HomebrewInstallRequest<'_>,
    prepared: &CandidateInstall,
) -> Result<(PathBuf, KegState), OrchestratorError> {
    let rack = Path::new(HOMEBREW_CELLAR).join(&prepared.formula);
    let install_started = epoch_seconds()?;
    run_brew(
        HomebrewCommand::TrustFormula {
            formula: super::tap::formula_reference(&prepared.formula)?,
        },
        &prepared.home,
        &prepared.temp,
        request.deadline,
    )?;
    prepared.tap_formula.verify(request.deadline)?;
    run_brew(
        HomebrewCommand::InstallCandidate {
            recipe: prepared.tap_formula.path.clone(),
        },
        &prepared.home,
        &prepared.temp,
        request.deadline,
    )?;
    prepared.candidate_formula.verify(request.deadline)?;

    let keg = rack.join(request.literals.version());
    verify_trusted_path(&keg, true)?;
    let before = inspect_keg(request, &keg, &prepared.tap_formula.path, install_started)?;
    let recipe = keg.join(".brew").join(format!("{}.rb", prepared.formula));
    let archived_recipe = HeldFile::new(
        &recipe,
        request.formula_sha256,
        request.formula_bytes.len() as u64,
        request.deadline,
    )?;
    run_brew(
        HomebrewCommand::TestArchived { recipe },
        &prepared.home,
        &prepared.temp,
        request.deadline,
    )?;
    prepared.candidate_formula.verify(request.deadline)?;
    prepared.tap_formula.verify(request.deadline)?;
    archived_recipe.verify(request.deadline)?;
    let after = inspect_keg(request, &keg, &prepared.tap_formula.path, install_started)?;
    if before != after {
        return Err(invalid("installed_keg_changed_during_test"));
    }

    Ok((keg, after))
}

fn finish_fresh_install(
    request: &HomebrewInstallRequest<'_>,
    prepared: &CandidateInstall,
    keg: PathBuf,
    state: KegState,
) -> Result<(HomebrewInstallEvidence, HomebrewInstalledKegCustody), OrchestratorError> {
    prepared.tap_formula.verify(request.deadline)?;
    prepared.candidate_formula.verify(request.deadline)?;
    let mut evidence = evidence_from_state(
        &prepared.formula,
        request,
        &prepared.brew_version,
        &prepared.prefix,
        &prepared.cellar,
        &keg,
        &state,
    );
    evidence.verification_sha256 = evidence_digest(&evidence);
    let directory = open_keg_directory(&keg)?;
    if snapshot_from_file(&directory)? != state.keg_identity
        || snapshot(&keg)? != state.keg_identity
    {
        return Err(invalid("installed_keg_custody_identity_mismatch"));
    }
    let custody = HomebrewInstalledKegCustody {
        keg,
        state,
        directory,
    };
    Ok((evidence, custody))
}
