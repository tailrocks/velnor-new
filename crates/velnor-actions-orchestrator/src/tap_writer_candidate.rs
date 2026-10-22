use super::helpers::is_sha256;
use super::*;
use std::{fs, path::Path};

pub(super) fn read_candidate_proof(
    root: &Path,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let dir = root.join(CANDIDATE_DIR);
    let proof_path = dir.join("candidate.json");
    let bytes = read_regular_limited(root, &proof_path, CANDIDATE_LIMIT)?;
    let value = velnor_actions_contract::parse_strict_json(
        std::str::from_utf8(&bytes).map_err(|_| internal("tap_candidate_proof_encoding"))?,
    )
    .map_err(|_| internal("tap_candidate_proof_json"))?;
    let proof: HomebrewTapPreparationProof =
        serde_json::from_value(value).map_err(|_| internal("tap_candidate_proof_schema"))?;
    if canonical_json_bytes(&proof).map_err(|_| internal("tap_candidate_proof_encode"))? != bytes {
        return Err(internal("tap_candidate_proof_noncanonical"));
    }
    Ok(proof)
}

pub(super) fn validate_proof(
    proof: &HomebrewTapPreparationProof,
    policy: &HomebrewTapConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPackageRelease,
) -> Result<(), OrchestratorError> {
    if proof.schema != "velnor.homebrew-candidate.v3"
        || proof.channel != plan.channel
        || proof.candidate_formula != "formula.rb"
        || proof.formula_path != crate::tap_prepare::formula_path(policy, plan.channel)?
        || proof.tap_repository != policy.repository
        || proof.source_repository != release.receipt.repository.slug
        || proof.source_sha != release.receipt.source.commit_sha
        || proof.version != release.receipt.version.value
        || proof.receipt_digest != release.receipt_digest
        || proof.plan_digest
            != sha256(&canonical_json_bytes(plan).map_err(|_| internal("tap_plan_encode"))?)
        || proof.tap_config_digest
            != sha256(&canonical_json_bytes(policy).map_err(|_| internal("tap_policy_encode"))?)
        || !is_sha256(&proof.old_formula_sha256)
        || !is_sha256(&proof.new_formula_sha256)
        || match proof.disposition {
            super::super::TapPreparationDisposition::Changed => {
                proof.old_formula_sha256 == proof.new_formula_sha256
            }
            super::super::TapPreparationDisposition::FormulaAlreadyCurrent => {
                proof.old_formula_sha256 != proof.new_formula_sha256
            }
        }
    {
        return Err(internal("tap_candidate_proof_binding_mismatch"));
    }
    release
        .validate(&proof.source_repository, plan)
        .map_err(|_| internal("tap_candidate_release_invalid"))
}

pub(super) fn read_regular_limited(
    root: &Path,
    path: &Path,
    limit: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| internal("tap_writer_path_escape"))?;
    let relative = relative
        .to_str()
        .ok_or_else(|| internal("tap_writer_path_encoding"))?;
    crate::check_evidence::reject_link_components(root, relative)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| internal("tap_writer_file_missing"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(internal("tap_writer_file_invalid"));
    }
    fs::read(path).map_err(|_| internal("tap_writer_file_read"))
}
