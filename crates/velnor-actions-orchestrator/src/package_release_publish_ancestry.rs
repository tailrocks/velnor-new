// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use serde_json::Value;

use super::{GitHubApi, is_lowercase_git_sha};
use crate::{OrchestratorError, internal::internal};

const MAX_COMPARE_COMMITS: u64 = 1_000_000;

#[derive(Deserialize)]
struct PublicBranch {
    name: String,
    protected: bool,
    commit: PublicBranchCommit,
}

#[derive(Deserialize)]
struct PublicBranchCommit {
    sha: String,
}

pub(in crate::package_release_publish) fn verify_public_preview_main_ancestry(
    api: &GitHubApi,
    repository_id: u64,
    branch: &str,
    source_sha: &str,
    workflow_authority_sha: &str,
) -> Result<String, OrchestratorError> {
    if repository_id == 0
        || branch != "main"
        || !is_lowercase_git_sha(source_sha)
        || !is_lowercase_git_sha(workflow_authority_sha)
    {
        return Err(internal("package_public_preview_ancestry_input_invalid"));
    }
    api.verify_preview_repository_identity(repository_id, branch)?;
    let captured_tip = public_protected_tip(api, branch)?;
    for sha in [source_sha, workflow_authority_sha, captured_tip.as_str()] {
        verify_commit_identity(api, sha, "package_public_preview_commit_mismatch")?;
    }
    require_ancestor(api, source_sha, workflow_authority_sha)?;
    require_ancestor(api, workflow_authority_sha, &captured_tip)?;
    let observed_tip = public_protected_tip(api, branch)?;
    if observed_tip != captured_tip {
        return Err(internal("package_public_preview_tip_changed"));
    }
    Ok(captured_tip)
}

pub(in crate::package_release_publish) fn verify_public_pull_request_lineage(
    api: &GitHubApi,
    repository_id: u64,
    branch: &str,
    base_sha: &str,
    head_sha: &str,
) -> Result<(), OrchestratorError> {
    if repository_id == 0
        || branch != "main"
        || !is_lowercase_git_sha(base_sha)
        || !is_lowercase_git_sha(head_sha)
    {
        return Err(internal("package_public_pr_ancestry_input_invalid"));
    }
    api.verify_preview_repository_identity(repository_id, branch)?;
    let captured_tip = public_protected_tip(api, branch)?;
    if captured_tip != base_sha {
        return Err(internal("package_public_pr_base_not_current"));
    }
    verify_commit_identity(api, head_sha, "package_public_pr_head_mismatch")?;
    require_ancestor(api, base_sha, head_sha)?;
    if public_protected_tip(api, branch)? != captured_tip {
        return Err(internal("package_public_pr_base_changed"));
    }
    Ok(())
}

fn public_protected_tip(api: &GitHubApi, branch: &str) -> Result<String, OrchestratorError> {
    let route = api.route(&format!("branches/{}", super::encode_segment(branch)));
    let response: PublicBranch = api.get_json(&route)?;
    if response.name != branch || !response.protected || !is_lowercase_git_sha(&response.commit.sha)
    {
        return Err(internal("package_public_main_not_protected"));
    }
    Ok(response.commit.sha)
}

pub(super) fn verify_preview_main_tip(
    api: &GitHubApi,
    repository_id: u64,
    branch: &str,
) -> Result<String, OrchestratorError> {
    verify_preview_main_tip_with(api, repository_id, branch, || {
        super::preflight::verify_main_protection(api, branch)
    })
}

fn verify_preview_main_tip_with<F>(
    api: &GitHubApi,
    repository_id: u64,
    branch: &str,
    mut capture_protected_tip: F,
) -> Result<String, OrchestratorError>
where
    F: FnMut() -> Result<String, OrchestratorError>,
{
    if repository_id == 0 || branch != "main" {
        return Err(internal("package_publish_preview_tip_input_invalid"));
    }
    api.verify_preview_repository_identity(repository_id, branch)?;
    let tip = capture_protected_tip()?;
    if !is_lowercase_git_sha(&tip) {
        return Err(internal("package_publish_main_source_invalid"));
    }
    let observed = capture_protected_tip()?;
    if observed != tip {
        return Err(internal("package_publish_main_ref_changed"));
    }
    Ok(tip)
}

pub(super) fn verify_preview_main_ancestry(
    api: &GitHubApi,
    repository_id: u64,
    branch: &str,
    source_sha: &str,
    workflow_authority_sha: &str,
) -> Result<String, OrchestratorError> {
    verify_preview_main_ancestry_with(
        api,
        repository_id,
        branch,
        source_sha,
        workflow_authority_sha,
        || super::preflight::verify_main_protection(api, branch),
    )
}

fn verify_preview_main_ancestry_with<F>(
    api: &GitHubApi,
    repository_id: u64,
    branch: &str,
    source_sha: &str,
    workflow_authority_sha: &str,
    mut capture_protected_tip: F,
) -> Result<String, OrchestratorError>
where
    F: FnMut() -> Result<String, OrchestratorError>,
{
    if repository_id == 0
        || branch != "main"
        || !is_lowercase_git_sha(source_sha)
        || !is_lowercase_git_sha(workflow_authority_sha)
    {
        return Err(internal("package_publish_preview_ancestry_input_invalid"));
    }

    api.verify_preview_repository_identity(repository_id, branch)?;
    let captured_tip = capture_protected_tip()?;
    if !is_lowercase_git_sha(&captured_tip) {
        return Err(internal("package_publish_main_source_invalid"));
    }

    for sha in [source_sha, workflow_authority_sha, captured_tip.as_str()] {
        verify_commit_identity(api, sha, "package_publish_preview_commit_mismatch")?;
    }
    require_ancestor(api, source_sha, workflow_authority_sha)?;
    require_ancestor(api, workflow_authority_sha, &captured_tip)?;

    let observed_tip = capture_protected_tip()?;
    if observed_tip != captured_tip {
        return Err(internal("package_publish_main_ref_changed"));
    }
    Ok(captured_tip)
}

fn verify_commit_identity(
    api: &GitHubApi,
    expected_sha: &str,
    mismatch: &'static str,
) -> Result<(), OrchestratorError> {
    let bytes = api.request_accept(
        crate::github_authority_read::arguments(&api.route(&format!("commits/{expected_sha}")))?,
        super::JSON_LIMIT,
        "application/vnd.github.sha",
    )?;
    let identity = bytes.strip_suffix(b"\n").unwrap_or(bytes.as_slice());
    let identity = std::str::from_utf8(identity)
        .ok()
        .filter(|identity| is_lowercase_git_sha(identity));
    if identity != Some(expected_sha) {
        return Err(internal(mismatch));
    }
    Ok(())
}

fn require_ancestor(api: &GitHubApi, base: &str, head: &str) -> Result<(), OrchestratorError> {
    if base == head {
        return Ok(());
    }

    let response: Value =
        api.get_json(&api.route(&format!("compare/{base}...{head}?per_page=1&page=2")))?;
    let reported_base = response.pointer("/base_commit/sha").and_then(Value::as_str);
    let merge_base = response
        .pointer("/merge_base_commit/sha")
        .and_then(Value::as_str);
    let status = response.get("status").and_then(Value::as_str);
    let ahead_by = response.get("ahead_by").and_then(Value::as_u64);
    let count = response.get("total_commits").and_then(Value::as_u64);
    let (Some(reported_base), Some(merge_base), Some(status), Some(ahead_by), Some(count)) =
        (reported_base, merge_base, status, ahead_by, count)
    else {
        return Err(internal("package_publish_preview_compare_invalid"));
    };
    if reported_base != base
        || merge_base != base
        || status != "ahead"
        || ahead_by != count
        || count == 0
        || count > MAX_COMPARE_COMMITS
    {
        return Err(internal("package_publish_preview_ancestry_rejected"));
    }
    if count == 1 {
        let commit: Value = api.get_json(&api.route(&format!("git/commits/{head}")))?;
        let parents = commit.get("parents").and_then(Value::as_array);
        if commit.get("sha").and_then(Value::as_str) != Some(head)
            || !parents.is_some_and(|parents| {
                parents
                    .iter()
                    .any(|parent| parent.get("sha").and_then(Value::as_str) == Some(base))
            })
        {
            return Err(internal("package_publish_preview_compare_head_mismatch"));
        }
        return Ok(());
    }
    let response: Value =
        api.get_json(&api.route(&format!("compare/{base}...{head}?per_page=1&page={count}")))?;
    if response.pointer("/commits/0/sha").and_then(Value::as_str) != Some(head) {
        return Err(internal("package_publish_preview_compare_head_mismatch"));
    }
    Ok(())
}
