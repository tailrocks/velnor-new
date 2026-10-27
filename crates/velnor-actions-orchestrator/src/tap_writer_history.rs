// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use velnor_actions_contract::canonical_json_bytes;

use super::{
    BotIdentity, OrchestratorError, TapGitHubApi, WriterInputs,
    github::{BlobResponse, CommitResponse, PullRequestResponse},
    gitops::{expected_pr_body, expected_pr_title, signed_off_message},
    helpers::{branch_name, is_full_sha, sha256},
    internal,
};
use crate::tap_prepare::reconstruct_historical_candidate_proof;
use crate::tap_writer_history_timestamp as timestamp;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;
const TAP_BASE: &str = "main";

#[path = "tap_writer_history_landed.rs"]
mod landed;
#[path = "tap_writer_history_selection.rs"]
mod selection;

pub(super) struct HistoricalPromotion {
    pub(super) pull_request: u64,
    pub(super) head_sha: String,
    pub(super) base_sha: String,
    pub(super) branch: String,
    pub(super) candidate_proof_digest: String,
    pub(super) merge_sha: String,
    pub(super) merged_at: String,
}

pub(super) fn find_already_promoted(
    inputs: &WriterInputs,
    bot: &BotIdentity,
) -> Result<Option<HistoricalPromotion>, OrchestratorError> {
    let pulls = all_pulls(&inputs.api)?;
    let receipt_marker = format!("Receipt digest: `{}`", inputs.release.receipt_digest);
    let branch_prefix = receipt_branch_prefix(
        &inputs.release.receipt_digest,
        inputs.release.receipt.channel,
    )?;
    let expected_title = expected_pr_title(&inputs.release);
    let mut candidates = Vec::new();
    for pull in pulls {
        if selection::relevant(
            &pull,
            bot.repository_id,
            &inputs.policy.repository,
            &branch_prefix,
            &receipt_marker,
            &expected_title,
        )? {
            candidates.push(pull);
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    if candidates.iter().any(|pull| pull.state == "open") {
        return Err(internal("tap_already_current_open_pr_exists"));
    }
    let mut authenticated = Vec::new();
    for summary in candidates {
        if summary.state != "closed" {
            return Err(internal("tap_historical_pr_state_invalid"));
        }
        let detail: PullRequestResponse = inputs
            .api
            .get_json(&inputs.api.repo_route(&format!("pulls/{}", summary.number)))?;
        if detail.number != summary.number || detail.merged.is_none() {
            return Err(internal("tap_historical_pr_detail_invalid"));
        }
        if detail.merged != Some(true) {
            return Err(internal("tap_already_current_unmerged_pr_exists"));
        }
        authenticated.push(authenticate_merged_pull(inputs, bot, &detail)?);
    }
    if authenticated.len() != 1 {
        return Err(internal("tap_already_current_historical_pr_count"));
    }
    authenticated
        .pop()
        .map(Some)
        .ok_or_else(|| internal("tap_already_current_historical_pr_missing"))
}

fn receipt_branch_prefix(
    receipt_digest: &str,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Result<String, OrchestratorError> {
    let digest = receipt_digest
        .strip_prefix("b3-")
        .filter(|value| super::helpers::is_sha256(value))
        .ok_or_else(|| internal("tap_historical_receipt_digest_invalid"))?;
    let channel = match channel {
        velnor_actions_contract::package_release::PackageReleaseChannel::Preview => "preview",
        velnor_actions_contract::package_release::PackageReleaseChannel::Stable => "stable",
    };
    Ok(format!("velnor/{channel}/{digest}-"))
}

fn all_pulls(api: &TapGitHubApi) -> Result<Vec<PullRequestResponse>, OrchestratorError> {
    let mut pulls = Vec::new();
    let mut seen = BTreeSet::new();
    for page in 1..=MAX_PAGES {
        let route = format!(
            "{}?state=all&per_page={PAGE_SIZE}&page={page}",
            api.repo_route("pulls")
        );
        let batch: Vec<PullRequestResponse> = api.get_json(&route)?;
        let len = batch.len();
        if len > PAGE_SIZE
            || batch
                .iter()
                .any(|pull| pull.number == 0 || !seen.insert(pull.number))
        {
            return Err(internal("tap_historical_pull_page_invalid"));
        }
        pulls.extend(batch);
        if selection::page_finished(len, page)? {
            return Ok(pulls);
        }
    }
    Err(internal("tap_historical_pull_listing_limit"))
}

fn authenticate_merged_pull(
    inputs: &WriterInputs,
    bot: &BotIdentity,
    pull: &PullRequestResponse,
) -> Result<HistoricalPromotion, OrchestratorError> {
    let owner = inputs
        .policy
        .repository
        .split_once('/')
        .map(|(owner, _)| owner)
        .ok_or_else(|| internal("tap_historical_repository_slug"))?;
    if pull.number == 0
        || pull.state != "closed"
        || pull.merged != Some(true)
        || pull.user.id != bot.user_id
        || pull.user.login != bot.login
        || pull.base.ref_name != TAP_BASE
        || !same_repository(pull, bot.repository_id, &inputs.policy.repository)
    {
        return Err(internal("tap_historical_pull_identity_mismatch"));
    }
    let head_sha = &pull.head.sha;
    if !is_full_sha(head_sha) {
        return Err(internal("tap_historical_head_sha_invalid"));
    }
    let head_commit = commit(&inputs.api, head_sha)?;
    if head_commit.parents.len() != 1 || !is_full_sha(&head_commit.parents[0].sha) {
        return Err(internal("tap_historical_head_parent_invalid"));
    }
    let base_sha = &head_commit.parents[0].sha;
    let branch = branch_name(
        &inputs.release.receipt_digest,
        base_sha,
        inputs.release.receipt.channel,
    )?;
    if pull.head.ref_name != branch
        || pull.head.label != format!("{owner}:{branch}")
        || pull.base.label != format!("{owner}:{TAP_BASE}")
    {
        return Err(internal("tap_historical_branch_mismatch"));
    }
    let proof_digest =
        authenticate_historical_candidate(inputs, bot, pull, base_sha, &head_commit)?;
    let (merge_sha, merged_at) = landed::merge_oid(inputs, bot, pull)?;
    validate_squash_commit(&inputs.api, &merge_sha, base_sha, &head_commit.tree.sha)?;
    Ok(HistoricalPromotion {
        pull_request: pull.number,
        head_sha: head_sha.clone(),
        base_sha: base_sha.clone(),
        branch,
        candidate_proof_digest: proof_digest,
        merge_sha,
        merged_at,
    })
}

fn authenticate_historical_candidate(
    inputs: &WriterInputs,
    bot: &BotIdentity,
    pull: &PullRequestResponse,
    base_sha: &str,
    head_commit: &CommitResponse,
) -> Result<String, OrchestratorError> {
    let base_commit = commit(&inputs.api, base_sha)?;
    let head_tree = read_tree(&inputs.api, &head_commit.tree.sha)?;
    let base_tree = read_tree(&inputs.api, &base_commit.tree.sha)?;
    validate_single_formula_delta(&inputs.formula_path, &base_tree, &head_tree)?;
    let old_formula = read_formula_blob(&inputs.api, &base_tree, &inputs.formula_path)?;
    let proposed = read_formula_blob(&inputs.api, &head_tree, &inputs.formula_path)?;
    if old_formula == proposed {
        return Err(internal("tap_historical_formula_not_changed"));
    }
    let proof =
        reconstruct_historical_candidate_proof(crate::tap_prepare::HistoricalCandidateInput {
            source_repository: &inputs.proof.source_repository,
            tap: &inputs.policy,
            plan: &inputs.plan,
            release: &inputs.release,
            spec: inputs.spec.as_ref(),
            sdk_artifact_digest: &inputs.sdk_artifact_digest,
            base_sha,
            base_formula: &old_formula,
            proposed_formula: &proposed,
        })?;
    let proof_digest =
        sha256(&canonical_json_bytes(&proof).map_err(|_| internal("tap_historical_proof_encode"))?);
    if proof_digest != proof_digest_from_body(pull)?
        || pull.body.as_deref()
            != Some(expected_pr_body(&inputs.release, &proof_digest, base_sha).as_str())
        || head_commit.message != signed_off_message(&proof, bot)
    {
        return Err(internal("tap_historical_candidate_proof_mismatch"));
    }
    Ok(proof_digest)
}

fn same_repository(pull: &PullRequestResponse, repository_id: u64, repository: &str) -> bool {
    pull.head
        .repo
        .as_ref()
        .is_some_and(|repo| repo.id == repository_id && repo.full_name == repository)
        && pull
            .base
            .repo
            .as_ref()
            .is_some_and(|repo| repo.id == repository_id && repo.full_name == repository)
}

fn proof_digest_from_body(pull: &PullRequestResponse) -> Result<String, OrchestratorError> {
    let body = pull
        .body
        .as_deref()
        .ok_or_else(|| internal("tap_historical_pr_body_missing"))?;
    let line = body
        .lines()
        .find(|line| line.starts_with("Candidate proof digest: `"))
        .ok_or_else(|| internal("tap_historical_proof_digest_missing"))?;
    let value = line
        .strip_prefix("Candidate proof digest: `")
        .and_then(|value| value.strip_suffix('`'))
        .ok_or_else(|| internal("tap_historical_proof_digest_malformed"))?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(internal("tap_historical_proof_digest_malformed"));
    }
    Ok(value.to_owned())
}

#[derive(Debug, Deserialize)]
struct GithubTree {
    sha: String,
    truncated: bool,
    tree: Vec<GithubTreeEntry>,
}

#[derive(Debug, Deserialize)]
struct GithubTreeEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    object_type: String,
    sha: String,
}

fn commit(api: &TapGitHubApi, sha: &str) -> Result<CommitResponse, OrchestratorError> {
    let commit: CommitResponse = api.get_json(&api.repo_route(&format!("git/commits/{sha}")))?;
    if commit.sha != sha || !is_full_sha(&commit.tree.sha) {
        return Err(internal("tap_historical_commit_invalid"));
    }
    Ok(commit)
}

fn read_tree(api: &TapGitHubApi, sha: &str) -> Result<GithubTree, OrchestratorError> {
    let tree: GithubTree =
        api.get_json(&api.repo_route(&format!("git/trees/{sha}?recursive=1")))?;
    if tree.sha != sha || tree.truncated {
        return Err(internal("tap_historical_tree_invalid"));
    }
    validate_tree_inventory(&tree)?;
    Ok(tree)
}

fn validate_tree_inventory(tree: &GithubTree) -> Result<(), OrchestratorError> {
    let entries = tree
        .tree
        .iter()
        .map(|entry| crate::homebrew::tree_validation::Entry {
            path: &entry.path,
            mode: &entry.mode,
            kind: &entry.object_type,
            sha: &entry.sha,
        })
        .collect::<Vec<_>>();
    crate::homebrew::tree_validation::validate(&entries, 100_000)
}

fn validate_single_formula_delta(
    formula: &str,
    base: &GithubTree,
    head: &GithubTree,
) -> Result<(), OrchestratorError> {
    let base_entries = base
        .tree
        .iter()
        .map(|entry| crate::homebrew::tree_validation::Entry {
            path: &entry.path,
            mode: &entry.mode,
            kind: &entry.object_type,
            sha: &entry.sha,
        })
        .collect::<Vec<_>>();
    let head_entries = head
        .tree
        .iter()
        .map(|entry| crate::homebrew::tree_validation::Entry {
            path: &entry.path,
            mode: &entry.mode,
            kind: &entry.object_type,
            sha: &entry.sha,
        })
        .collect::<Vec<_>>();
    crate::homebrew::tree_validation::validate_formula_delta(
        &base_entries,
        &head_entries,
        formula,
        100_000,
    )
}

fn read_formula_blob(
    api: &TapGitHubApi,
    tree: &GithubTree,
    formula: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    let entry = tree
        .tree
        .iter()
        .find(|entry| entry.path == formula)
        .ok_or_else(|| internal("tap_historical_formula_missing"))?;
    let blob: BlobResponse = api.get_json(&api.repo_route(&format!("git/blobs/{}", entry.sha)))?;
    if blob.sha != entry.sha || blob.encoding != "base64" {
        return Err(internal("tap_historical_blob_invalid"));
    }
    STANDARD
        .decode(blob.content.replace('\n', ""))
        .map_err(|_| internal("tap_historical_blob_encoding"))
}

fn validate_squash_commit(
    api: &TapGitHubApi,
    merge_sha: &str,
    original_base: &str,
    candidate_tree: &str,
) -> Result<(), OrchestratorError> {
    let merged = commit(api, merge_sha)?;
    if merged.tree.sha != candidate_tree
        || merged.parents.len() != 1
        || merged.parents[0].sha != original_base
    {
        return Err(internal("tap_historical_squash_commit_mismatch"));
    }
    Ok(())
}
