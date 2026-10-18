//! Reauthenticate the signed release and fixed tap preparation handoff.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPreviewRelease, canonical_json_bytes,
    config::HomebrewTapConfig,
    homebrew_formula::{HomebrewFormulaCandidate, validate_homebrew_formula_candidate},
    parse_strict_json,
};

use crate::{
    OrchestratorError,
    internal::internal,
    tap_prepare::{HomebrewTapPreparationProof, TapPromotionReceipt},
};

const LIMIT: u64 = 1024 * 1024;

pub(super) struct MergeCandidate {
    pub(super) policy: HomebrewTapConfig,
    pub(super) plan: PackageReleasePlan,
    pub(super) release: VerifiedPreviewRelease,
    pub(super) proof: HomebrewTapPreparationProof,
    pub(super) promotion: TapPromotionReceipt,
    pub(super) formula: Vec<u8>,
}

pub(super) fn load_candidate() -> Result<MergeCandidate, OrchestratorError> {
    let signed = crate::package_release_signer::verify_attested_signed_package_from_env()?;
    let plan = signed.plan().clone();
    plan.validate()
        .map_err(|_| internal("tap_merge_plan_invalid"))?;
    let receipt = signed.receipt().clone();
    let release = VerifiedPreviewRelease {
        receipt_digest: receipt
            .canonical_digest()
            .map_err(|_| internal("tap_merge_receipt_digest"))?,
        receipt,
    };
    release
        .validate(&plan.repository, &plan)
        .map_err(|_| internal("tap_merge_receipt_invalid"))?;
    let policy = crate::tap_prepare::writer_policy_from_env()?;
    let root = runner_root()?;
    let proof: HomebrewTapPreparationProof =
        read_canonical_json(&root.join("tap-candidate/candidate.json"))?;
    let formula = read_regular(&root, &root.join("tap-candidate/formula.rb"))?;
    let promotion: TapPromotionReceipt = read_canonical_json(&root.join("tap-pr.json"))?;
    validate_handoff(&policy, &plan, &release, &proof, &promotion, &formula)?;
    Ok(MergeCandidate {
        policy,
        plan,
        release,
        proof,
        promotion,
        formula,
    })
}

fn validate_handoff(
    policy: &HomebrewTapConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPreviewRelease,
    proof: &HomebrewTapPreparationProof,
    promotion: &TapPromotionReceipt,
    formula: &[u8],
) -> Result<(), OrchestratorError> {
    let proof_bytes =
        canonical_json_bytes(proof).map_err(|_| internal("tap_merge_proof_encode"))?;
    let plan_digest = sha256(
        &plan
            .canonical_bytes()
            .map_err(|_| internal("tap_merge_plan_encode"))?,
    );
    let tap_digest =
        sha256(&canonical_json_bytes(policy).map_err(|_| internal("tap_merge_policy_encode"))?);
    if proof.schema != "velnor.homebrew-candidate.v1"
        || proof.formula_path != policy.formula
        || proof.candidate_formula != "formula.rb"
        || proof.tap_repository != policy.repository
        || proof.source_repository != plan.repository
        || proof.source_sha != release.receipt.source.commit_sha
        || proof.version != release.receipt.version.value
        || proof.receipt_digest != release.receipt_digest
        || proof.plan_digest != plan_digest
        || proof.tap_config_digest != tap_digest
        || proof.new_formula_sha256 != sha256(formula)
        || promotion.schema != "velnor.homebrew-tap-pr.v1"
        || promotion.repository != policy.repository
        || promotion.base_sha != proof.base_sha
        || promotion.candidate_proof_digest != sha256(&proof_bytes)
        || promotion.release_receipt_digest != release.receipt_digest
        || promotion.branch != expected_branch(&release.receipt_digest, &proof.base_sha)?
        || promotion.pull_request == 0
        || !is_hex(&promotion.base_sha, 40)
        || !is_hex(&promotion.head_sha, 40)
    {
        return Err(internal("tap_merge_handoff_binding_mismatch"));
    }
    Ok(())
}

pub(super) fn validate_formula(
    candidate: &MergeCandidate,
    base_sha: &str,
    base_formula: &[u8],
) -> Result<(), OrchestratorError> {
    if base_sha != candidate.proof.base_sha
        || sha256(base_formula) != candidate.proof.old_formula_sha256
    {
        return Err(internal("tap_merge_base_formula_mismatch"));
    }
    let canonical = validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository: &candidate.proof.source_repository,
        plan: &candidate.plan,
        tap_repository: &candidate.policy.repository,
        formula_path: &candidate.policy.formula,
        captured_base_sha: base_sha,
        base_bytes: base_formula,
        proposed_bytes: &candidate.formula,
        release: &candidate.release,
    })
    .map_err(|_| internal("tap_merge_formula_rejected"))?;
    if canonical != candidate.formula {
        return Err(internal("tap_merge_formula_noncanonical"));
    }
    Ok(())
}

pub(super) fn expected_branch(
    receipt_digest: &str,
    base_sha: &str,
) -> Result<String, OrchestratorError> {
    let digest = receipt_digest
        .strip_prefix("b3-")
        .filter(|value| is_hex(value, 64))
        .ok_or_else(|| internal("tap_merge_branch_digest_invalid"))?;
    if !is_hex(base_sha, 40) {
        return Err(internal("tap_merge_branch_base_invalid"));
    }
    Ok(format!("velnor/preview/{digest}-{base_sha}"))
}

fn read_canonical_json<T: DeserializeOwned + serde::Serialize>(
    path: &Path,
) -> Result<T, OrchestratorError> {
    let root = runner_root()?;
    let bytes = read_regular(&root, path)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| internal("tap_merge_json_utf8"))?;
    let value = parse_strict_json(text).map_err(|_| internal("tap_merge_json_invalid"))?;
    let parsed: T = serde_json::from_value(value).map_err(|_| internal("tap_merge_json_schema"))?;
    if canonical_json_bytes(&parsed).map_err(|_| internal("tap_merge_json_encode"))? != bytes {
        return Err(internal("tap_merge_json_noncanonical"));
    }
    Ok(parsed)
}

fn runner_root() -> Result<PathBuf, OrchestratorError> {
    let root = PathBuf::from(
        env::var_os("RUNNER_TEMP").ok_or_else(|| internal("tap_merge_temp_missing"))?,
    )
    .join("velnor");
    if !root.is_absolute()
        || !fs::symlink_metadata(&root)
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
    {
        return Err(internal("tap_merge_temp_invalid"));
    }
    Ok(root)
}

fn read_regular(root: &Path, path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| internal("tap_merge_path_escape"))?;
    let relative = relative
        .to_str()
        .ok_or_else(|| internal("tap_merge_path_encoding"))?;
    crate::check_evidence::reject_link_components(root, relative)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| internal("tap_merge_file_missing"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > LIMIT
    {
        return Err(internal("tap_merge_file_invalid"));
    }
    fs::read(path).map_err(|_| internal("tap_merge_file_read"))
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            out
        })
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
