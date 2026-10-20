//! Canonical preparation proof construction and persistence.

use std::path::Path;

use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPackageRelease, canonical_json_bytes,
    config::HomebrewTapConfig,
    homebrew_formula::{HomebrewFormulaCandidate, validate_homebrew_formula_candidate},
};

use super::{
    HomebrewTapPreparationProof, OrchestratorError, TapPreparationDisposition, internal, sha256,
};

const CANDIDATE_DIRECTORY: &str = "tap-candidate";

#[derive(Clone, Copy)]
pub(super) struct CandidateIdentity<'a> {
    pub(super) source_repository: &'a str,
    pub(super) tap: &'a HomebrewTapConfig,
    pub(super) release: &'a VerifiedPackageRelease,
    pub(super) base_sha: &'a str,
    pub(super) old_sha: &'a str,
    pub(super) formula: &'a [u8],
    pub(super) plan: &'a PackageReleasePlan,
    pub(super) formula_path: &'a str,
    pub(super) disposition: TapPreparationDisposition,
}

pub(super) fn persist_candidate(
    temp: &Path,
    identity: CandidateIdentity<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let directory = temp.join(CANDIDATE_DIRECTORY);
    crate::exclusive_write::create_dir_no_symlink(temp, &directory)?;
    let formula_path = directory.join("formula.rb");
    crate::exclusive_write::write_exclusive(
        &formula_path,
        identity.formula,
        "tap_candidate_formula",
    )?;
    let proof = candidate_proof(&identity)?;
    let bytes = canonical_json_bytes(&proof).map_err(|_| internal("tap_candidate_proof_encode"))?;
    crate::exclusive_write::write_exclusive(
        &directory.join("candidate.json"),
        &bytes,
        "tap_candidate_proof",
    )?;
    Ok(proof)
}

/// Reconstruct the original changed proof from trusted release and formula inputs.
/// This supports historical PR authentication when fresh tap main is already current.
pub(crate) fn reconstruct_historical_candidate_proof(
    source_repository: &str,
    tap: &HomebrewTapConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPackageRelease,
    base_sha: &str,
    base_formula: &[u8],
    proposed_formula: &[u8],
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let formula_path = super::formula_path(tap, plan.channel)?;
    let canonical = validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository,
        tap_repository: &tap.repository,
        formula_path,
        captured_base_sha: base_sha,
        base_bytes: base_formula,
        proposed_bytes: proposed_formula,
        plan,
        release,
    })
    .map_err(|_| internal("tap_historical_formula_rejected"))?;
    candidate_proof(&CandidateIdentity {
        source_repository,
        tap,
        release,
        base_sha,
        old_sha: &sha256(base_formula),
        formula: &canonical,
        plan,
        formula_path,
        disposition: TapPreparationDisposition::Changed,
    })
}

fn candidate_proof(
    candidate: &CandidateIdentity<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let CandidateIdentity {
        source_repository,
        tap,
        release,
        base_sha,
        old_sha,
        formula,
        plan,
        formula_path,
        disposition,
    } = candidate;
    Ok(HomebrewTapPreparationProof {
        schema: "velnor.homebrew-candidate.v3".to_owned(),
        channel: release.receipt.channel,
        disposition: *disposition,
        base_sha: (*base_sha).to_owned(),
        formula_path: (*formula_path).to_owned(),
        old_formula_sha256: (*old_sha).to_owned(),
        new_formula_sha256: sha256(formula),
        receipt_digest: release.receipt_digest.clone(),
        plan_digest: sha256(&canonical_json_bytes(plan).map_err(|_| internal("tap_plan_encode"))?),
        tap_config_digest: sha256(
            &canonical_json_bytes(tap).map_err(|_| internal("tap_policy_encode"))?,
        ),
        tap_repository: tap.repository.clone(),
        source_repository: (*source_repository).to_owned(),
        source_sha: release.receipt.source.commit_sha.clone(),
        version: release.receipt.version.value.clone(),
        candidate_formula: "formula.rb".to_owned(),
    })
}
