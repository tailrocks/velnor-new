// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, authorize_github_candidate,
    internal, require, reviews::ReviewAuthor, valid_sha, valid_utc, verify_github_evidence,
    verify_reviews,
};
use crate::crate_release_qualification_source::{
    ReviewedPullState, ReviewedSourceRequest, verify_pull_source,
};

pub fn verify_oidc_qualification(
    request: &GitHubAdmissionRequest,
    reviewed: &ReviewedSourceRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    let ReviewedPullState::Merged { merge_sha, .. } = &reviewed.state else {
        return Err(internal("oidc_qualification_reviewed_merge"));
    };
    require(
        request.event == "workflow_dispatch"
            && request.workflow_path == ".github/workflows/release.yml"
            && request.workflow_sha == request.source_sha
            && request.recovery_sha.as_deref() == Some(request.source_sha.as_str())
            && request.source_sha == *merge_sha
            && request.repository == reviewed.repository
            && request.default_branch == reviewed.default_branch
            && request.required_workflow_path == reviewed.required_workflow_path,
        "oidc_qualification_binding",
    )?;
    let source = verify_pull_source(reviewed, api)?;
    let prefix = format!("repos/{}", request.repository);
    let route = format!("{prefix}/pulls/{}", reviewed.pull_request);
    let pull = api.get(&route)?;
    let author = ReviewAuthor {
        login: pull["user"]["login"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| internal("oidc_qualification_author"))?,
        id: pull["user"]["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("oidc_qualification_author"))?,
    };
    let merged = pull["merged_at"]
        .as_str()
        .filter(|s| valid_utc(s))
        .ok_or_else(|| internal("oidc_qualification_merge_time"))?;
    require(valid_sha(&reviewed.head_sha), "oidc_qualification_head")?;
    verify_reviews(
        api,
        &prefix,
        reviewed.pull_request,
        author,
        &reviewed.head_sha,
        merged,
    )?;
    let candidate = verify_github_evidence(request, api)?;
    let authorized = authorize_github_candidate(request, &candidate, api)?;
    require(
        authorized.repository_id() == reviewed.repository_id
            && verify_pull_source(reviewed, api)? == source
            && api.get(&route)? == pull,
        "oidc_qualification_changed",
    )?;
    verify_reviews(
        api,
        &prefix,
        reviewed.pull_request,
        author,
        &reviewed.head_sha,
        merged,
    )?;
    let fresh_authorized = authorize_github_candidate(request, &candidate, api)?;
    require(fresh_authorized == authorized, "oidc_qualification_changed")?;
    Ok(())
}

pub fn verify_oidc_qualification_for_run(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    let reviewed = reviewed_merge_for_run(request, api)?;
    verify_oidc_qualification(request, &reviewed, api)?;
    require(
        reviewed_merge_for_run(request, api)? == reviewed,
        "oidc_qualification_changed",
    )
}

fn reviewed_merge_for_run(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
) -> Result<ReviewedSourceRequest, OrchestratorError> {
    require(
        valid_sha(&request.workflow_sha)
            && velnor_actions_contract::vcs::is_safe_github_repository(&request.repository)
            && velnor_actions_contract::is_valid_branch_name(&request.default_branch),
        "oidc_qualification_context",
    )?;
    let prefix = format!("repos/{}", request.repository);
    let repository = api.get(&prefix)?;
    let repository_id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("oidc_qualification_repository"))?;
    require(
        repository["full_name"].as_str() == Some(request.repository.as_str())
            && repository["default_branch"].as_str() == Some(request.default_branch.as_str()),
        "oidc_qualification_repository",
    )?;
    let mut selected = None;
    for page in 1..=100 {
        let pulls = api.get(&format!(
            "{prefix}/commits/{}/pulls?per_page=100&page={page}",
            request.workflow_sha
        ))?;
        let pulls = pulls
            .as_array()
            .ok_or_else(|| internal("oidc_qualification_pulls"))?;
        for pull in pulls {
            if pull["merged_at"].is_null()
                || pull["merge_commit_sha"].as_str() != Some(request.workflow_sha.as_str())
                || pull["base"]["ref"].as_str() != Some(request.default_branch.as_str())
                || pull["base"]["repo"]["id"].as_u64() != Some(repository_id)
                || pull["base"]["repo"]["full_name"].as_str() != Some(request.repository.as_str())
            {
                continue;
            }
            require(selected.is_none(), "oidc_qualification_ambiguous_pull")?;
            let number = pull["number"]
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| internal("oidc_qualification_pull"))?;
            let head = pull["head"]["sha"]
                .as_str()
                .filter(|sha| valid_sha(sha))
                .ok_or_else(|| internal("oidc_qualification_head"))?;
            selected = Some((number, head.to_owned()));
        }
        if pulls.len() < 100 {
            break;
        }
        require(page < 100, "oidc_qualification_pulls_limit")?;
    }
    let (pull_request, head_sha) =
        selected.ok_or_else(|| internal("oidc_qualification_pull_missing"))?;
    let commit = api.get(&format!("{prefix}/git/commits/{}", request.workflow_sha))?;
    let tree = commit["tree"]["sha"]
        .as_str()
        .filter(|sha| valid_sha(sha))
        .ok_or_else(|| internal("oidc_qualification_tree"))?;
    require(
        commit["sha"].as_str() == Some(request.workflow_sha.as_str()),
        "oidc_qualification_commit",
    )?;
    Ok(ReviewedSourceRequest {
        repository: request.repository.clone(),
        repository_id,
        pull_request,
        head_sha,
        default_branch: request.default_branch.clone(),
        required_workflow_path: request.required_workflow_path.clone(),
        state: ReviewedPullState::Merged {
            merge_sha: request.workflow_sha.clone(),
            merge_tree_sha: tree.to_owned(),
        },
    })
}

pub fn oidc_qualification_admit_from_environment() -> Result<(), OrchestratorError> {
    let required = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| internal("oidc_qualification_context"))
    };
    let repository = required("GITHUB_REPOSITORY")?;
    let default_branch = required("VELNOR_QUALIFICATION_DEFAULT_BRANCH")?;
    let environment = required("VELNOR_QUALIFICATION_ENVIRONMENT")?;
    let required_workflow_path = required("VELNOR_QUALIFICATION_REQUIRED_WORKFLOW")?;
    let workflow_sha = required("GITHUB_SHA")?;
    let workflow_path = velnor_actions_contract::CRATE_RELEASE_WORKFLOW_PATH.to_owned();
    let event = required("GITHUB_EVENT_NAME")?;
    require(
        velnor_actions_contract::vcs::is_safe_github_repository(&repository)
            && velnor_actions_contract::is_valid_branch_name(&default_branch)
            && valid_sha(&workflow_sha)
            && event == "workflow_dispatch"
            && required("VELNOR_QUALIFICATION_MODE")? == "oidc-qualification"
            && required("GITHUB_REF")? == format!("refs/heads/{default_branch}")
            && required("GITHUB_WORKFLOW_SHA")? == workflow_sha
            && required("GITHUB_WORKFLOW_REF")?
                == format!("{repository}/{workflow_path}@refs/heads/{default_branch}"),
        "oidc_qualification_context",
    )?;
    let trusted_reviewer_id = required("VELNOR_QUALIFICATION_REVIEWER_ID")?
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("oidc_qualification_context"))?;
    let run_id = required("GITHUB_RUN_ID")?
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("oidc_qualification_context"))?;
    let run_attempt = required("GITHUB_RUN_ATTEMPT")?
        .parse::<u32>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("oidc_qualification_context"))?;
    let request = GitHubAdmissionRequest {
        repository,
        default_branch,
        environment,
        trusted_reviewer_id,
        workflow_path,
        required_workflow_path,
        workflow_sha: workflow_sha.clone(),
        run_id,
        run_attempt,
        event,
        source_sha: workflow_sha.clone(),
        recovery_sha: Some(workflow_sha),
    };
    let api = super::NativeGitHubAdmissionRead::new(required("GIT_TOKEN")?)?;
    verify_oidc_qualification_for_run(&request, &api)
}
