// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubAdmissionRead, GitHubAdmissionRequest, GitHubCandidate, NativeGitHubAdmissionRead,
    OrchestratorError, Value, encode, internal, require, valid_sha, validate_request,
    verify_environment, verify_run_authority,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAuthorized {
    candidate: GitHubCandidate,
    repository_id: u64,
    owner_id: u64,
}

impl GitHubAuthorized {
    #[must_use]
    pub fn candidate(&self) -> &GitHubCandidate {
        &self.candidate
    }

    #[must_use]
    pub fn repository_id(&self) -> u64 {
        self.repository_id
    }

    #[must_use]
    pub fn owner_id(&self) -> u64 {
        self.owner_id
    }
}

pub fn authorize_github_candidate(
    request: &GitHubAdmissionRequest,
    candidate: &GitHubCandidate,
    api: &dyn GitHubAdmissionRead,
) -> Result<GitHubAuthorized, OrchestratorError> {
    let identity = repository_identity(request, api)?;
    let fresh =
        super::verify_github_evidence_with_check(request, api, Some(candidate.required_check_id))?;
    require(&fresh == candidate, "candidate_changed")?;
    verify_environment_approval(request, api)?;
    require(
        repository_identity(request, api)? == identity,
        "repository_identity_changed",
    )?;
    Ok(GitHubAuthorized {
        candidate: fresh,
        repository_id: identity.0,
        owner_id: identity.1,
    })
}

fn repository_identity(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<(u64, u64), OrchestratorError> {
    validate_request(request)?;
    let prefix = format!("repos/{}", request.repository);
    let repository = api.get(&prefix)?;
    let run = api.get(&format!("{prefix}/actions/runs/{}", request.run_id))?;
    let repo_id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_repository_id"))?;
    let owner_id = repository["owner"]["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_owner_id"))?;
    let owner = request
        .repository
        .split_once('/')
        .ok_or_else(|| internal("crate_release_repository"))?
        .0;
    require(
        repository["full_name"].as_str() == Some(request.repository.as_str())
            && repository["owner"]["login"].as_str() == Some(owner),
        "repository_identity",
    )?;
    require(
        run["id"].as_u64() == Some(request.run_id)
            && run["run_attempt"].as_u64() == Some(u64::from(request.run_attempt)),
        "run_identity",
    )?;
    for field in ["repository", "head_repository"] {
        require(
            run[field]["id"].as_u64() == Some(repo_id)
                && run[field]["full_name"].as_str() == Some(request.repository.as_str())
                && run[field]["owner"]["id"].as_u64() == Some(owner_id)
                && run[field]["owner"]["login"].as_str() == Some(owner),
            "workflow_repository_identity",
        )?;
    }
    Ok((repo_id, owner_id))
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
        repository_info["full_name"].as_str().is_some_and(|actual| {
            crate::release::release_identity::same_repository(actual, repository)
        }),
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
