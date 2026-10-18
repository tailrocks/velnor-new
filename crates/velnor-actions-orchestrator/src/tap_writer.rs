//! Credential-isolated writer for a validated Homebrew tap candidate.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes,
    config::HomebrewTapConfig,
    homebrew_formula::{
        HomebrewFormulaCandidate, VerifiedPreviewRelease, validate_homebrew_formula_candidate,
    },
};

use crate::{
    OrchestratorError, internal::internal,
    package_release_signer::verify_attested_signed_package_from_env,
    tap_prepare::HomebrewTapPreparationProof,
};

#[path = "tap_writer_github.rs"]
mod github;
#[path = "tap_writer_gitops.rs"]
mod gitops;
#[path = "tap_writer_identity.rs"]
mod identity;
#[path = "tap_writer_receipt.rs"]
mod receipt;
#[path = "tap_writer_result.rs"]
mod result;
use github::{
    BlobResponse, CommitResponse, RefResponse, TapGitHubApi, TreeResponse, encode_segment,
};
use gitops::{
    BranchRequest, PullRequestRequest, create_or_reconcile_branch, create_or_reconcile_pr,
};
use identity::{BotIdentity, verify_app_scope};
pub use receipt::{TapPromotionOutcome, TapPromotionReceipt};
use result::persist_result;
#[path = "tap_writer_helpers.rs"]
mod helpers;
#[path = "tap_writer_history.rs"]
mod history;
use helpers::{
    branch_name, is_full_sha, is_sha256, parse_decimal_id, real_directory, required_env,
    safe_app_slug, sha256,
};

const CANDIDATE_LIMIT: u64 = 1024 * 1024;
const CANDIDATE_DIR: &str = "tap-candidate";
const PR_RECEIPT_FILE: &str = "tap-pr.json";

/// Private writer operation selected only by the generated App-authenticated job.
pub const WRITE_HOMEBREW_TAP_PR_OP: &str = "write-homebrew-tap-pr-v1";

/// Reverify the source release and independently validate/write one PR candidate.
/// # Errors
/// Rejects a missing policy, an invalid candidate, a mismatched App installation,
/// a stale tap base, a conflicting retry, or any GitHub API failure.
pub fn write_homebrew_tap_pr_from_env() -> Result<TapPromotionReceipt, OrchestratorError> {
    let inputs = load_writer_inputs()?;
    let bot = verify_app_scope(
        &inputs.api,
        &inputs.policy,
        inputs.app_id,
        inputs.installation_id,
        &inputs.app_slug,
    )?;
    let base_sha = verify_repository_base(&inputs.api, &inputs.policy, &inputs.proof)?;
    let base_tree = read_formula_base(&inputs.api, &inputs.policy.formula, &base_sha)?;
    if sha256(&base_tree.bytes) != inputs.proof.old_formula_sha256 {
        return Err(internal("tap_candidate_base_digest_mismatch"));
    }
    let canonical = validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository: &inputs.proof.source_repository,
        plan: &inputs.plan,
        tap_repository: &inputs.policy.repository,
        formula_path: &inputs.policy.formula,
        captured_base_sha: &base_sha,
        base_bytes: &base_tree.bytes,
        proposed_bytes: &inputs.proposed,
        release: &inputs.release,
    })
    .map_err(|_| internal("tap_writer_formula_rejected"))?;
    if canonical != inputs.proposed {
        return Err(internal("tap_writer_formula_noncanonical"));
    }
    let preparation_proof_digest =
        sha256(&canonical_json_bytes(&inputs.proof).map_err(|_| internal("tap_proof_encode"))?);
    if let Some(result) =
        already_promoted(&inputs, &bot, &base_tree.bytes, &preparation_proof_digest)?
    {
        return Ok(result);
    }
    let branch = branch_name(&inputs.release.receipt_digest, &base_sha)?;
    let mut verify_current = || verify_promotion_authority(&inputs);
    let head_sha = create_or_reconcile_branch(
        &inputs.api,
        &mut BranchRequest {
            policy: &inputs.policy,
            proof: &inputs.proof,
            formula: &inputs.proposed,
            base_sha: &base_sha,
            base_tree_sha: &base_tree.tree_sha,
            branch: &branch,
            bot: &bot,
            verify_current: &mut verify_current,
        },
    )?;
    let pull_request = create_or_reconcile_pr(
        &inputs.api,
        &mut PullRequestRequest {
            policy: &inputs.policy,
            branch: &branch,
            head_sha: &head_sha,
            base_sha: &base_sha,
            proof_digest: &preparation_proof_digest,
            release: &inputs.release,
            bot: &bot,
            verify_current: &mut verify_current,
        },
    )?;
    let result = TapPromotionReceipt {
        schema: "velnor.homebrew-tap-pr.v2".to_owned(),
        repository: inputs.policy.repository.clone(),
        release_receipt_digest: inputs.release.receipt_digest,
        preparation_proof_digest: preparation_proof_digest.clone(),
        outcome: TapPromotionOutcome::PullRequest {
            pull_request,
            head_sha,
            base_sha,
            branch,
            candidate_proof_digest: preparation_proof_digest,
        },
    };
    persist_result(&inputs.root, &result)?;
    Ok(result)
}

fn already_promoted(
    inputs: &WriterInputs,
    bot: &BotIdentity,
    current_formula: &[u8],
    preparation_proof_digest: &str,
) -> Result<Option<TapPromotionReceipt>, OrchestratorError> {
    if inputs.proof.disposition != super::TapPreparationDisposition::FormulaAlreadyCurrent {
        return Ok(None);
    }
    if inputs.proposed != current_formula
        || inputs.proof.old_formula_sha256 != inputs.proof.new_formula_sha256
    {
        return Err(internal("tap_already_current_formula_mismatch"));
    }
    let historical = history::find_already_promoted(inputs, bot)?;
    let result = TapPromotionReceipt {
        schema: "velnor.homebrew-tap-pr.v2".to_owned(),
        repository: inputs.policy.repository.clone(),
        release_receipt_digest: inputs.release.receipt_digest.clone(),
        preparation_proof_digest: preparation_proof_digest.to_owned(),
        outcome: TapPromotionOutcome::AlreadyPromoted {
            pull_request: historical.pull_request,
            head_sha: historical.head_sha,
            base_sha: historical.base_sha,
            branch: historical.branch,
            candidate_proof_digest: historical.candidate_proof_digest,
            merge_sha: historical.merge_sha,
            merged_at: historical.merged_at,
        },
    };
    persist_result(&inputs.root, &result)?;
    Ok(Some(result))
}

fn verify_promotion_authority(inputs: &WriterInputs) -> Result<(), OrchestratorError> {
    crate::package_release_publish::verify_tap_update_environment(
        &inputs.plan.repository,
        &inputs.plan.default_branch,
    )?;
    crate::tap_source_freshness::verify_preview_source_is_current(
        &inputs.plan.repository,
        &inputs.plan.default_branch,
        &inputs.release.receipt.source.commit_sha,
    )
}

struct WriterInputs {
    root: PathBuf,
    policy: HomebrewTapConfig,
    plan: PackageReleasePlan,
    release: VerifiedPreviewRelease,
    proof: HomebrewTapPreparationProof,
    proposed: Vec<u8>,
    api: TapGitHubApi,
    app_id: u64,
    installation_id: u64,
    app_slug: String,
}

fn load_writer_inputs() -> Result<WriterInputs, OrchestratorError> {
    let signed = verify_attested_signed_package_from_env()?;
    let plan = signed.plan().clone();
    let receipt = signed.receipt().clone();
    let release = VerifiedPreviewRelease {
        receipt_digest: receipt
            .canonical_digest()
            .map_err(|_| internal("tap_writer_receipt_digest"))?,
        receipt,
    };
    let root = runner_root()?;
    let policy = super::execute::static_tap_policy()?;
    let proof = read_candidate_proof(&root)?;
    validate_proof(&proof, &policy, &plan, &release)?;
    let candidate_path = root.join(CANDIDATE_DIR).join("formula.rb");
    let proposed = read_regular_limited(&root, &candidate_path, CANDIDATE_LIMIT)?;
    if sha256(&proposed) != proof.new_formula_sha256 {
        return Err(internal("tap_candidate_content_digest_mismatch"));
    }
    let token = required_env("VELNOR_TAP_TOKEN")?;
    if env::var("GH_TOKEN").is_ok_and(|source_token| source_token == token) {
        return Err(internal("tap_writer_app_token_equals_source_token"));
    }
    let app_id = parse_decimal_id(&required_env("VELNOR_TAP_APP_ID")?)?;
    let installation_id = parse_decimal_id(&required_env("VELNOR_TAP_INSTALLATION_ID")?)?;
    let app_slug_value = required_env("VELNOR_TAP_APP_SLUG")?;
    safe_app_slug(&app_slug_value)?;
    let api = TapGitHubApi::new(policy.repository.clone(), token);
    Ok(WriterInputs {
        root,
        policy,
        plan,
        release,
        proof,
        proposed,
        api,
        app_id,
        installation_id,
        app_slug: app_slug_value,
    })
}

fn runner_root() -> Result<PathBuf, OrchestratorError> {
    let path = PathBuf::from(required_env("RUNNER_TEMP")?).join("velnor");
    if !path.is_absolute() || !real_directory(&path) {
        return Err(internal("tap_writer_runner_temp_invalid"));
    }
    Ok(path)
}

fn read_candidate_proof(root: &Path) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
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

fn validate_proof(
    proof: &HomebrewTapPreparationProof,
    policy: &HomebrewTapConfig,
    plan: &PackageReleasePlan,
    release: &VerifiedPreviewRelease,
) -> Result<(), OrchestratorError> {
    if proof.schema != "velnor.homebrew-candidate.v2"
        || proof.candidate_formula != "formula.rb"
        || proof.formula_path != policy.formula
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
            super::TapPreparationDisposition::Changed => {
                proof.old_formula_sha256 == proof.new_formula_sha256
            }
            super::TapPreparationDisposition::FormulaAlreadyCurrent => {
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

fn read_regular_limited(
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

fn verify_repository_base(
    api: &TapGitHubApi,
    policy: &HomebrewTapConfig,
    proof: &HomebrewTapPreparationProof,
) -> Result<String, OrchestratorError> {
    if policy.repository != proof.tap_repository {
        return Err(internal("tap_writer_target_mismatch"));
    }
    let current: RefResponse = api.get_json(&api.repo_route("git/ref/heads/main"))?;
    if current.object.object_type != "commit" || current.object.sha != proof.base_sha {
        return Err(internal("tap_writer_stale_tap_main"));
    }
    Ok(current.object.sha)
}

struct FormulaBase {
    bytes: Vec<u8>,
    tree_sha: String,
}

fn read_formula_base(
    api: &TapGitHubApi,
    formula: &str,
    base_sha: &str,
) -> Result<FormulaBase, OrchestratorError> {
    let commit: CommitResponse =
        api.get_json(&api.repo_route(&format!("git/commits/{base_sha}")))?;
    if commit.sha != base_sha || !is_full_sha(&commit.tree.sha) {
        return Err(internal("tap_writer_base_commit_invalid"));
    }
    let tree: TreeResponse =
        api.get_json(&api.repo_route(&format!("git/trees/{}?recursive=1", commit.tree.sha)))?;
    if tree.truncated || tree.sha != commit.tree.sha {
        return Err(internal("tap_writer_tree_truncated"));
    }
    let mut entries = tree.tree.iter().filter(|entry| entry.path == formula);
    let entry = entries
        .next()
        .ok_or_else(|| internal("tap_writer_formula_missing"))?;
    if entries.next().is_some() || entry.mode != "100644" || entry.object_type != "blob" {
        return Err(internal("tap_writer_formula_mode_invalid"));
    }
    let blob: BlobResponse = api.get_json(&api.repo_route(&format!("git/blobs/{}", entry.sha)))?;
    if blob.sha != entry.sha || blob.encoding != "base64" {
        return Err(internal("tap_writer_formula_blob_invalid"));
    }
    let bytes = STANDARD
        .decode(blob.content.replace('\n', ""))
        .map_err(|_| internal("tap_writer_formula_blob_encoding"))?;
    Ok(FormulaBase {
        bytes,
        tree_sha: tree.sha,
    })
}

#[cfg(test)]
#[path = "tap_writer_retry_tests.rs"]
mod retry_tests;
#[cfg(test)]
#[path = "tap_writer_tests.rs"]
mod tests;
