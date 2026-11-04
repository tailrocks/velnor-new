// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubAdmissionRead, GitHubAdmissionRequest, GitHubCandidate, NativeGitHubAdmissionRead,
    OrchestratorError, Value, encode, internal, require, valid_sha, validate_request,
    verify_environment, verify_github_evidence, verify_run_authority,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAuthorized {
    candidate: GitHubCandidate,
}

impl GitHubAuthorized {
    #[must_use]
    pub fn candidate(&self) -> &GitHubCandidate {
        &self.candidate
    }
}

pub fn authorize_github_candidate(
    request: &GitHubAdmissionRequest,
    candidate: &GitHubCandidate,
    api: &dyn GitHubAdmissionRead,
) -> Result<GitHubAuthorized, OrchestratorError> {
    let fresh = verify_github_evidence(request, api)?;
    require(&fresh == candidate, "candidate_changed")?;
    verify_environment_approval(request, api)?;
    Ok(GitHubAuthorized { candidate: fresh })
}

pub fn verify_environment_approval(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    validate_request(request)?;
    require(request.run_attempt == 1, "approval_attempt_unverified")?;
    let prefix = format!("repos/{}", request.repository);
    verify_run_authority(request, api, &prefix)?;
    let id = verify_environment(request, api, &prefix)?;
    let response = api.get(&format!(
        "{prefix}/actions/runs/{}/approvals",
        request.run_id
    ))?;
    let approvals = response
        .as_array()
        .ok_or_else(|| internal("crate_release_approvals_invalid"))?;
    let related: Vec<&Value> = approvals
        .iter()
        .filter(|approval| {
            approval["environments"].as_array().is_some_and(|envs| {
                envs.iter().any(|env| {
                    env["id"].as_u64() == Some(id)
                        && env["name"].as_str() == Some(request.environment.as_str())
                })
            })
        })
        .collect();
    require(related.len() == 1, "environment_approval_count")?;
    let approval = related[0];
    let expected = format!(
        "I reviewed the complete release policy at {} and source {}, and authorize this source for run {}.",
        request.workflow_sha, request.source_sha, request.run_id
    );
    require(
        approval["state"].as_str() == Some("approved")
            && approval["user"]["type"].as_str() == Some("User")
            && approval["user"]["id"].as_u64() == Some(request.trusted_reviewer_id)
            && approval["comment"].as_str() == Some(expected.as_str()),
        "environment_approval",
    )
}

pub fn read_github_tag(
    repository: &str,
    tag: &str,
    token: String,
) -> Result<Option<String>, OrchestratorError> {
    let api = NativeGitHubAdmissionRead::new(token)?;
    read_github_tag_evidence(repository, tag, &api)
}

pub fn read_github_tag_evidence(
    repository: &str,
    tag: &str,
    api: &dyn GitHubAdmissionRead,
) -> Result<Option<String>, OrchestratorError> {
    let Some((owner, repo)) = repository.split_once('/') else {
        return Err(internal("crate_release_repository"));
    };
    require(
        velnor_actions_contract::config::is_valid_github_owner(owner)
            && velnor_actions_contract::config::is_valid_github_repo_name(repo),
        "repository",
    )?;
    require(
        !tag.is_empty()
            && !tag.starts_with('.')
            && !tag.ends_with('.')
            && tag.rsplit_once('.').is_none_or(|(_, ext)| ext != "lock")
            && !tag.contains("..")
            && tag
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+')),
        "tag",
    )?;
    let prefix = format!("repos/{repository}");
    let repository_info: Value = api.get(&prefix)?;
    require(
        repository_info["full_name"].as_str() == Some(repository),
        "repository",
    )?;
    let branch = repository_info["default_branch"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("crate_release_default_branch"))?;
    let branch_ref = api.get(&format!("{prefix}/git/ref/heads/{}", encode(branch)))?;
    require(
        branch_ref["ref"].as_str() == Some(format!("refs/heads/{branch}").as_str())
            && branch_ref["object"]["type"].as_str() == Some("commit")
            && branch_ref["object"]["sha"].as_str().is_some_and(valid_sha),
        "branch_ref",
    )?;
    let Some(reference): Option<Value> =
        api.get_optional(&format!("{prefix}/git/ref/tags/{}", encode(tag)))?
    else {
        return Ok(None);
    };
    require(
        reference["ref"].as_str() == Some(format!("refs/tags/{tag}").as_str()),
        "tag_reference",
    )?;
    let mut object = reference["object"].clone();
    for _ in 0..=8 {
        let sha = object["sha"]
            .as_str()
            .filter(|s| valid_sha(s))
            .ok_or_else(|| internal("crate_release_tag_sha"))?;
        match object["type"].as_str() {
            Some("commit") => return Ok(Some(sha.to_owned())),
            Some("tag") => {
                let annotated: Value = api.get(&format!("{prefix}/git/tags/{sha}"))?;
                require(annotated["sha"].as_str() == Some(sha), "tag_object")?;
                object = annotated["object"].clone();
            }
            _ => return Err(internal("crate_release_tag_type")),
        }
    }
    Err(internal("crate_release_tag_depth"))
}
