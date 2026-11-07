// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubAdmissionRead, GitHubAdmissionRequest, OrchestratorError, Value, encode, internal,
    require,
};

pub(super) fn verify_run_authority(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
) -> Result<(), OrchestratorError> {
    let repo: Value = api.get(prefix)?;
    require(
        repo["full_name"].as_str() == Some(request.repository.as_str()),
        "repository",
    )?;
    require(
        repo["default_branch"].as_str() == Some(request.default_branch.as_str()),
        "default_branch",
    )?;
    let run: Value = api.get(&format!("{prefix}/actions/runs/{}", request.run_id))?;
    require(run["id"].as_u64() == Some(request.run_id), "run_id")?;
    require(
        run["run_attempt"].as_u64() == Some(u64::from(request.run_attempt)),
        "run_attempt",
    )?;
    require(
        run["head_sha"].as_str() == Some(request.workflow_sha.as_str()),
        "workflow_sha",
    )?;
    require(
        run["head_branch"].as_str() == Some(request.default_branch.as_str()),
        "workflow_branch",
    )?;
    require(
        run["event"].as_str() == Some(request.event.as_str()),
        "event",
    )?;
    require(
        run["path"].as_str() == Some(request.workflow_path.as_str()),
        "workflow_path",
    )?;
    require(
        run["repository"]["full_name"].as_str() == Some(request.repository.as_str()),
        "workflow_repository",
    )?;
    require(
        run["head_repository"]["full_name"].as_str() == Some(request.repository.as_str()),
        "workflow_head_repository",
    )?;
    let compare: Value = api.get(&format!(
        "{prefix}/compare/{}...{}",
        request.source_sha, request.workflow_sha
    ))?;
    require(
        matches!(compare["status"].as_str(), Some("ahead" | "identical")),
        "source_ancestry",
    )?;
    let workflow_compare: Value = api.get(&format!(
        "{prefix}/compare/{}...{}",
        request.workflow_sha,
        encode(&request.default_branch)
    ))?;
    require(
        matches!(
            workflow_compare["status"].as_str(),
            Some("ahead" | "identical")
        ),
        "workflow_ancestry",
    )?;
    Ok(())
}

pub(super) fn verify_environment(
    request: &GitHubAdmissionRequest,
    api: &dyn GitHubAdmissionRead,
    prefix: &str,
) -> Result<u64, OrchestratorError> {
    let env: Value = api.get(&format!(
        "{prefix}/environments/{}",
        encode(&request.environment)
    ))?;
    require(
        env["name"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(&request.environment)),
        "environment",
    )?;
    let id = env["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| internal("crate_release_environment_id"))?;
    require(
        env["deployment_branch_policy"]["custom_branch_policies"].as_bool() == Some(true),
        "environment_policy",
    )?;
    require(
        env["can_admins_bypass"].as_bool() == Some(false),
        "environment_bypass",
    )?;
    let protections = env["protection_rules"]
        .as_array()
        .ok_or_else(|| internal("crate_release_environment_protection"))?;
    let reviewers: Vec<&Value> = protections
        .iter()
        .filter(|rule| rule["type"].as_str() == Some("required_reviewers"))
        .collect();
    require(reviewers.len() == 1, "environment_reviewer_rule")?;
    let allowed = reviewers[0]["reviewers"]
        .as_array()
        .ok_or_else(|| internal("crate_release_environment_reviewers"))?;
    require(
        allowed.len() == 1
            && allowed[0]["type"].as_str() == Some("User")
            && allowed[0]["reviewer"]["id"].as_u64() == Some(request.trusted_reviewer_id),
        "environment_reviewers",
    )?;

    let policies: Value = api.get(&format!(
        "{prefix}/environments/{}/deployment-branch-policies?per_page=100",
        encode(&request.environment)
    ))?;
    require(
        policies["total_count"].as_u64() == Some(1),
        "environment_policy_count",
    )?;
    let rules = policies["branch_policies"]
        .as_array()
        .ok_or_else(|| internal("crate_release_environment_rules"))?;
    require(
        rules.len() == 1
            && rules[0]["type"].as_str() == Some("branch")
            && rules[0]["name"].as_str() == Some(request.default_branch.as_str()),
        "environment_branch",
    )?;
    Ok(id)
}
