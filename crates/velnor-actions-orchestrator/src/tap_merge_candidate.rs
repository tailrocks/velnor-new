// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPackageRelease, canonical_json_bytes, config::HomebrewTapConfig,
    homebrew_spec::HomebrewFormulaSpec, parse_strict_json,
};

use crate::{
    OrchestratorError,
    internal::internal,
    tap_prepare::{
        HomebrewTapPreparationProof, TapPreparationDisposition, TapPromotionOutcome,
        TapPromotionReceipt,
    },
};

const LIMIT: u64 = 1024 * 1024;

pub(super) struct MergeCandidate {
    pub(super) policy: HomebrewTapConfig,
    pub(super) spec: Option<HomebrewFormulaSpec>,
    pub(super) sdk_artifact_digest: String,
    pub(super) plan: PackageReleasePlan,
    pub(super) release: VerifiedPackageRelease,
    pub(super) proof: HomebrewTapPreparationProof,
    pub(super) promotion: TapPromotionReceipt,
    pub(super) formula: Vec<u8>,
    pub(super) formula_path: String,
}

pub(super) struct PromotionSelection<'a> {
    pub(super) pull_request: u64,
    pub(super) head_sha: &'a str,
    pub(super) base_sha: &'a str,
    pub(super) branch: &'a str,
    pub(super) candidate_proof_digest: &'a str,
    pub(super) merged: Option<(&'a str, &'a str)>,
}

impl MergeCandidate {
    pub(super) fn selection(&self) -> Option<PromotionSelection<'_>> {
        Some(match &self.promotion.outcome {
            TapPromotionOutcome::PullRequest {
                pull_request,
                head_sha,
                base_sha,
                branch,
                candidate_proof_digest,
            } => PromotionSelection {
                pull_request: *pull_request,
                head_sha,
                base_sha,
                branch,
                candidate_proof_digest,
                merged: None,
            },
            TapPromotionOutcome::AlreadyPromoted {
                pull_request,
                head_sha,
                base_sha,
                branch,
                candidate_proof_digest,
                merge_sha,
                merged_at,
            } => PromotionSelection {
                pull_request: *pull_request,
                head_sha,
                base_sha,
                branch,
                candidate_proof_digest,
                merged: Some((merge_sha, merged_at)),
            },
            TapPromotionOutcome::SkippedStaleSource { .. } => return None,
        })
    }

    pub(super) fn selection_for(&self, pull_request: u64) -> Option<PromotionSelection<'_>> {
        let selection = self.selection()?;
        (selection.pull_request == pull_request).then_some(selection)
    }
}

pub(super) fn load_candidate() -> Result<MergeCandidate, OrchestratorError> {
    let signed = crate::package_release_signer::verify_attested_signed_package_from_env()?;
    crate::package_release_publish::verify_published_release(&signed)?;
    let plan = signed.plan().clone();
    plan.validate()
        .map_err(|_| internal("tap_merge_plan_invalid"))?;
    crate::tap_prepare::release_policy_from_env()?
        .validate_plan(&plan)
        .map_err(|_| internal("tap_merge_release_policy_plan"))?;
    let receipt = signed.receipt().clone();
    let release = VerifiedPackageRelease {
        receipt_digest: receipt
            .canonical_digest()
            .map_err(|_| internal("tap_merge_receipt_digest"))?,
        receipt,
    };
    release
        .validate(&plan.repository, &plan)
        .map_err(|_| internal("tap_merge_receipt_invalid"))?;
    let policy = crate::tap_prepare::writer_policy_from_env()?;
    let spec = crate::tap_prepare::formula_policy_from_env()?;
    let sdk_artifact_digest = crate::generator::current_exe_sha256()
        .ok_or_else(|| internal("tap_sdk_artifact_digest_unavailable"))?;
    let formula_path = crate::tap_prepare::formula_path(&policy, plan.channel)?.to_owned();
    let root = runner_root()?;
    let proof: HomebrewTapPreparationProof =
        read_canonical_json(&root.join("tap-candidate/candidate.json"))?;
    let formula = read_regular(&root, &root.join("tap-candidate/formula.rb"))?;
    let promotion: TapPromotionReceipt = read_canonical_json(&root.join("tap-pr.json"))?;
    validate_handoff(&HandoffValidation {
        policy: &policy,
        spec: spec.as_ref(),
        sdk_artifact_digest: &sdk_artifact_digest,
        plan: &plan,
        release: &release,
        proof: &proof,
        promotion: &promotion,
        formula: &formula,
    })?;
    Ok(MergeCandidate {
        policy,
        spec,
        sdk_artifact_digest,
        plan,
        release,
        proof,
        promotion,
        formula,
        formula_path,
    })
}

struct HandoffValidation<'a> {
    policy: &'a HomebrewTapConfig,
    spec: Option<&'a HomebrewFormulaSpec>,
    sdk_artifact_digest: &'a str,
    plan: &'a PackageReleasePlan,
    release: &'a VerifiedPackageRelease,
    proof: &'a HomebrewTapPreparationProof,
    promotion: &'a TapPromotionReceipt,
    formula: &'a [u8],
}

fn validate_handoff(input: &HandoffValidation<'_>) -> Result<(), OrchestratorError> {
    let HandoffValidation {
        policy,
        spec,
        sdk_artifact_digest,
        plan,
        release,
        proof,
        promotion,
        formula,
    } = input;
    let proof_bytes =
        canonical_json_bytes(proof).map_err(|_| internal("tap_merge_proof_encode"))?;
    let plan_digest = sha256(
        &plan
            .canonical_bytes()
            .map_err(|_| internal("tap_merge_plan_encode"))?,
    );
    let tap_digest =
        sha256(&canonical_json_bytes(policy).map_err(|_| internal("tap_merge_policy_encode"))?);
    let formula_spec_digest = spec
        .map(|spec| {
            spec.digest()
                .map_err(|_| internal("tap_formula_spec_digest"))
        })
        .transpose()?;
    if proof.schema != "velnor.homebrew-candidate.v4"
        || proof.channel != plan.channel
        || proof.formula_path != crate::tap_prepare::formula_path(policy, plan.channel)?
        || proof.candidate_formula != "formula.rb"
        || proof.tap_repository != policy.repository
        || proof.source_repository != plan.repository
        || proof.source_sha != release.receipt.source.commit_sha
        || proof.version != release.receipt.version.value
        || proof.receipt_digest != release.receipt_digest
        || proof.plan_digest != plan_digest
        || proof.formula_spec_digest != formula_spec_digest
        || proof.sdk_artifact_digest != *sdk_artifact_digest
        || proof.tap_config_digest != tap_digest
        || proof.new_formula_sha256 != sha256(formula)
        || promotion.schema != "velnor.homebrew-tap-pr.v2"
        || promotion.repository != policy.repository
        || promotion.release_receipt_digest != release.receipt_digest
        || promotion.preparation_proof_digest != sha256(&proof_bytes)
    {
        return Err(internal("tap_merge_handoff_binding_mismatch"));
    }
    validate_promotion_outcome(input)
}

fn validate_promotion_outcome(input: &HandoffValidation<'_>) -> Result<(), OrchestratorError> {
    let HandoffValidation {
        proof,
        promotion,
        release,
        ..
    } = input;
    match &promotion.outcome {
        TapPromotionOutcome::PullRequest {
            pull_request,
            head_sha,
            base_sha,
            branch,
            candidate_proof_digest,
        } if proof.disposition == TapPreparationDisposition::Changed
            && *pull_request > 0
            && base_sha == &proof.base_sha
            && branch
                == &expected_branch(
                    &release.receipt_digest,
                    base_sha,
                    release.receipt.channel,
                )?
            && candidate_proof_digest == &promotion.preparation_proof_digest
            && is_hex(head_sha, 40)
            && is_hex(base_sha, 40) => {}
        TapPromotionOutcome::AlreadyPromoted {
            pull_request,
            head_sha,
            base_sha,
            branch,
            candidate_proof_digest,
            merge_sha,
            merged_at,
        } if proof.disposition == TapPreparationDisposition::FormulaAlreadyCurrent
            && proof.old_formula_sha256 == proof.new_formula_sha256
            && *pull_request > 0
            && branch
                == &expected_branch(
                    &release.receipt_digest,
                    base_sha,
                    release.receipt.channel,
                )?
            && is_hex(candidate_proof_digest, 64)
            && is_hex(head_sha, 40)
            && is_hex(base_sha, 40)
            && is_hex(merge_sha, 40)
            && !merged_at.is_empty() => {}
        TapPromotionOutcome::SkippedStaleSource { .. } => {
            return Err(internal("tap_merge_skipped_promotion_invalid"));
        }
        _ => return Err(internal("tap_merge_handoff_binding_mismatch")),
    }
    Ok(())
}

pub(super) fn validate_formula_at_base(
    candidate: &MergeCandidate,
    base_sha: &str,
    base_formula: &[u8],
    proposed_formula: &[u8],
) -> Result<(), OrchestratorError> {
    let canonical =
        crate::tap_prepare::derive_formula_candidate(crate::tap_prepare::FormulaCandidateInput {
            spec: candidate.spec.as_ref(),
            source_repository: &candidate.proof.source_repository,
            tap_repository: &candidate.policy.repository,
            formula_path: &candidate.formula_path,
            base_sha,
            base_formula,
            proposed_formula: Some(proposed_formula),
            plan: &candidate.plan,
            release: &candidate.release,
        })
        .map_err(|_| internal("tap_merge_formula_rejected"))?;
    if canonical != proposed_formula {
        return Err(internal("tap_merge_formula_noncanonical"));
    }
    Ok(())
}

pub(super) fn expected_branch(
    receipt_digest: &str,
    base_sha: &str,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Result<String, OrchestratorError> {
    let digest = receipt_digest
        .strip_prefix("b3-")
        .filter(|value| is_hex(value, 64))
        .ok_or_else(|| internal("tap_merge_branch_digest_invalid"))?;
    if !is_hex(base_sha, 40) {
        return Err(internal("tap_merge_branch_base_invalid"));
    }
    let channel = match channel {
        velnor_actions_contract::package_release::PackageReleaseChannel::Preview => "preview",
        velnor_actions_contract::package_release::PackageReleaseChannel::Stable => "stable",
    };
    Ok(format!("velnor/{channel}/{digest}-{base_sha}"))
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

pub(super) fn sha256(bytes: &[u8]) -> String {
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
