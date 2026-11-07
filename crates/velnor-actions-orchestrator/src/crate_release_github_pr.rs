// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, internal, require, valid_sha,
    valid_utc, verify_reviews,
};

pub(super) fn approved_pr(
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
    request: &GitHubAdmissionRequest,
    commit_sha: &str,
) -> Result<u64, OrchestratorError> {
    let mut candidates = Vec::new();
    for page in 1..=100 {
        let response = api.get(&format!(
            "{prefix}/commits/{commit_sha}/pulls?per_page=100&page={page}"
        ))?;
        let prs = response
            .as_array()
            .ok_or_else(|| internal("crate_release_pulls_invalid"))?;
        let count = prs.len();
        for pr in prs {
            if pr["merged_at"].is_null() || pr["merge_commit_sha"].as_str() != Some(commit_sha) {
                continue;
            }
            if pr["base"]["ref"].as_str() != Some(request.default_branch.as_str())
                || pr["base"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
            {
                continue;
            }
            if pr["head"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
                || !pr["head"]["ref"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("release-plz-"))
            {
                continue;
            }
            candidates.push(pr.clone());
        }
        if count < 100 {
            break;
        }
        require(page < 100, "pulls_limit")?;
    }
    require(candidates.len() == 1, "release_pr")?;
    let pr = &candidates[0];
    let number = pr["number"]
        .as_u64()
        .ok_or_else(|| internal("crate_release_pr_number"))?;
    let author = pr["user"]["login"]
        .as_str()
        .ok_or_else(|| internal("crate_release_pr_author"))?;
    let author_id = pr["user"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_pr_author_id"))?;
    let author = super::reviews::ReviewAuthor {
        login: author,
        id: author_id,
    };
    let head = pr["head"]["sha"]
        .as_str()
        .ok_or_else(|| internal("crate_release_pr_head"))?;
    require(valid_sha(head), "release_pr_head")?;
    let merged = pr["merged_at"]
        .as_str()
        .filter(|value| valid_utc(value))
        .ok_or_else(|| internal("crate_release_merge_time"))?;
    let source_commit = api.get(&format!("{prefix}/git/commits/{commit_sha}"))?;
    let head_commit = api.get(&format!("{prefix}/git/commits/{head}"))?;
    let source_tree = source_commit["tree"]["sha"]
        .as_str()
        .filter(|s| valid_sha(s))
        .ok_or_else(|| internal("crate_release_source_tree"))?;
    require(
        source_commit["sha"].as_str() == Some(commit_sha)
            && head_commit["sha"].as_str() == Some(head)
            && head_commit["tree"]["sha"].as_str() == Some(source_tree),
        "reviewed_source_tree",
    )?;
    verify_reviews(api, prefix, number, author, head, merged)?;
    Ok(number)
}
