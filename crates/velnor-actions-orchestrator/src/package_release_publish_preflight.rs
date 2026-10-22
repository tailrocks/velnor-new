//! Fail-closed runtime checks for source repository and environment controls.

use std::{
    env,
    ffi::{OsStr, OsString},
    time::Duration,
};

use serde_json::Value;
use velnor_actions_contract::{
    package_release::PackageReleaseChannel, vcs::is_safe_github_repository,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec};

use super::{API_VERSION, GitHubApi, JSON_LIMIT};
use crate::{OrchestratorError, internal::internal};

const API_TIMEOUT: Duration = Duration::from_secs(90);
const PREVIEW_ENVIRONMENT: &str = "preview-publish";
const TAP_ENVIRONMENT: &str = "tap-update";
const STABLE_ENVIRONMENT: &str = "release";

pub(super) fn verify_main_protection(
    api: &GitHubApi,
    branch: &str,
) -> Result<String, OrchestratorError> {
    if branch != "main" {
        return Err(internal("package_publish_default_branch_not_main"));
    }
    let status_token = status_token()?;
    let rules_route = format!("repos/{}/rules/branches/main", api.repository);
    let rules = api_json(&api.catalog, &status_token, &api.repository, &rules_route)?;
    let classic_route = format!("repos/{}/branches/main/protection", api.repository);
    let classic =
        api_json_or_not_found(&api.catalog, &status_token, &api.repository, &classic_route)?;
    validate_main_protection_sources(&rules, classic.as_ref())
        .map_err(|()| internal("package_publish_main_protection_incomplete"))?;
    verify_named_environment(&api.repository, PREVIEW_ENVIRONMENT, branch)?;
    // This combined protection/source snapshot is the decisive final remote read.
    let branch_route = format!("repos/{}/branches/main", api.repository);
    let branch_json = api_json(&api.catalog, &status_token, &api.repository, &branch_route)?;
    let source_sha = protected_branch_sha(&branch_json)?;
    Ok(source_sha)
}

pub(super) fn verify_release_channel_environment(
    channel: PackageReleaseChannel,
    api: &GitHubApi,
    default_branch: &str,
) -> Result<Option<String>, OrchestratorError> {
    match channel {
        PackageReleaseChannel::Preview => {
            if default_branch != "main" {
                return Err(internal("package_publish_default_branch_not_main"));
            }
            verify_main_protection(api, default_branch).map(Some)
        }
        PackageReleaseChannel::Stable => {
            verify_stable_environment(&api.repository, default_branch).map(|()| None)
        }
    }
}

#[cfg(test)]
#[path = "package_release_publish_preflight_tests.rs"]
mod tests;

#[path = "package_release_publish_preflight_validation.rs"]
mod validation;
#[cfg(test)]
use validation::validate_effective_main_rules;
use validation::{
    validate_environment_branch_policy, validate_environment_settings,
    validate_main_protection_sources, validate_stable_environment_settings,
};

/// Verify the source repository's fixed `tap-update` environment before App writes or merge.
/// # Errors
/// Rejects a missing source token, wrong repository, human-review rules, or non-main policy.
fn verify_tap_update_environment(repository: &str, branch: &str) -> Result<(), OrchestratorError> {
    verify_named_environment(repository, TAP_ENVIRONMENT, branch)
}

fn verify_stable_environment(repository: &str, branch: &str) -> Result<(), OrchestratorError> {
    if branch.is_empty() || !is_safe_github_repository(repository) {
        return Err(internal("package_publish_environment_identity_invalid"));
    }
    let token = status_token()?;
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let route = format!(
        "repos/{repository}/environments/{}",
        encode_segment(STABLE_ENVIRONMENT)
    );
    let settings = api_json(&catalog, &token, repository, &route)?;
    validate_stable_environment_settings(&settings, branch)
        .map_err(|()| internal("package_publish_stable_environment_policy_invalid"))
}

pub(crate) fn tap_update_environment(
    repository: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    verify_tap_update_environment(repository, branch)
}

fn verify_named_environment(
    repository: &str,
    environment: &str,
    branch: &str,
) -> Result<(), OrchestratorError> {
    if branch != "main"
        || !matches!(environment, PREVIEW_ENVIRONMENT | TAP_ENVIRONMENT)
        || !is_safe_github_repository(repository)
    {
        return Err(internal("package_publish_environment_identity_invalid"));
    }
    let token = status_token()?;
    let catalog = velnor_actions_mise::ToolCatalog::pinned();
    let env_route = format!(
        "repos/{repository}/environments/{}",
        encode_segment(environment)
    );
    let settings = api_json(&catalog, &token, repository, &env_route)?;
    validate_environment_settings(&settings, environment)
        .map_err(|()| internal("package_publish_environment_policy_invalid"))?;
    let policies_route = format!(
        "repos/{repository}/environments/{}/deployment-branch-policies?per_page=100&page=1",
        encode_segment(environment)
    );
    let policies = api_json(&catalog, &token, repository, &policies_route)?;
    validate_environment_branch_policy(&policies, branch)
        .map_err(|()| internal("package_publish_environment_branch_policy_invalid"))
}

fn status_token() -> Result<String, OrchestratorError> {
    let token = super::immutable_status::immutable_status_token(
        env::var("VELNOR_RELEASE_STATUS_TOKEN").ok().as_deref(),
        env::var("GH_TOKEN").ok().as_deref(),
        env::var("VELNOR_TAP_TOKEN").ok().as_deref(),
    )?;
    if env::var("VELNOR_TAP_MERGE_TOKEN").ok().as_deref() == Some(token.as_str()) {
        return Err(internal("package_publish_immutable_status_token_reused"));
    }
    Ok(token)
}

pub(super) fn verify_repository_identity(
    api: &GitHubApi,
    expected_id: u64,
) -> Result<(), OrchestratorError> {
    let token = status_token()?;
    let response = api_json(
        &api.catalog,
        &token,
        &api.repository,
        &format!("repos/{}", api.repository),
    )?;
    if response.get("id").and_then(Value::as_u64) != Some(expected_id)
        || response.get("full_name").and_then(Value::as_str) != Some(api.repository.as_str())
    {
        return Err(internal("package_publish_repository_mismatch"));
    }
    Ok(())
}

fn protected_branch_sha(response: &Value) -> Result<String, OrchestratorError> {
    if response.get("protected").and_then(Value::as_bool) != Some(true)
        || response.get("name").and_then(Value::as_str) != Some("main")
    {
        return Err(internal("package_publish_main_not_protected"));
    }
    response
        .pointer("/commit/sha")
        .and_then(Value::as_str)
        .filter(|sha| super::super::identity::is_git_sha(sha))
        .map(str::to_owned)
        .ok_or_else(|| internal("package_publish_main_source_invalid"))
}

fn validate_status_route(repository: &str, route: &str) -> Result<(), OrchestratorError> {
    if !is_safe_github_repository(repository) {
        return Err(internal("package_status_repository_invalid"));
    }
    let base = format!("repos/{repository}");
    if route == base
        || [
            "branches/main",
            "rules/branches/main",
            "branches/main/protection",
        ]
        .iter()
        .any(|suffix| route == format!("{base}/{suffix}"))
    {
        return Ok(());
    }
    for environment in [PREVIEW_ENVIRONMENT, STABLE_ENVIRONMENT, TAP_ENVIRONMENT] {
        let path = format!("{base}/environments/{environment}");
        if route == path
            || route == format!("{path}/deployment-branch-policies?per_page=100&page=1")
        {
            return Ok(());
        }
    }
    Err(internal("package_status_route_not_allowed"))
}

fn api_json(
    catalog: &velnor_actions_mise::ToolCatalog,
    token: &str,
    repository: &str,
    route: &str,
) -> Result<Value, OrchestratorError> {
    validate_status_route(repository, route)?;
    let mut args = vec![
        OsString::from("api"),
        OsString::from("-H"),
        OsString::from("Accept: application/vnd.github+json"),
        OsString::from("-H"),
        OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
    ];
    args.extend(crate::github_authority_read::arguments(route)?);
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("package_publish_gh_unavailable"))?;
    let output = exec
        .command_with_github_token(catalog, token)
        .and_then(|command| command.run_bounded(JSON_LIMIT, API_TIMEOUT))
        .map_err(|_| internal("package_publish_settings_read_failed"))?;
    if !output.success {
        return Err(internal("package_publish_settings_read_failed"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| internal("package_publish_settings_response_invalid"))
}

fn api_json_or_not_found(
    catalog: &velnor_actions_mise::ToolCatalog,
    token: &str,
    repository: &str,
    route: &str,
) -> Result<Option<Value>, OrchestratorError> {
    validate_status_route(repository, route)?;
    let mut args = vec![
        OsString::from("api"),
        OsString::from("-H"),
        OsString::from("Accept: application/vnd.github+json"),
        OsString::from("-H"),
        OsString::from(format!("X-GitHub-Api-Version: {API_VERSION}")),
    ];
    args.extend(crate::github_authority_read::arguments(route)?);
    let exec = PinnedToolExec::new(vec![PinnedTool::Gh], OsStr::new("gh"), args)
        .map_err(|_| internal("package_publish_gh_unavailable"))?;
    let output = exec
        .command_with_github_token(catalog, token)
        .and_then(|command| command.run_bounded(JSON_LIMIT, API_TIMEOUT))
        .map_err(|_| internal("package_publish_settings_read_failed"))?;
    if output.success {
        return serde_json::from_slice(&output.stdout)
            .map(Some)
            .map_err(|_| internal("package_publish_settings_response_invalid"));
    }
    if classic_protection_absent(&output.stderr, &output.stdout) {
        return Ok(None);
    }
    Err(internal("package_publish_settings_read_failed"))
}

/// Recognize the exact GitHub answers for a branch without classic protection.
///
/// A caller with Administration read on a branch protected only by rulesets receives
/// `404 {"message":"Branch not protected"}`; the generic `404 {"message":"Not Found"}` is
/// also accepted. Both mean "no classic source": the effective rules read is still required
/// and must supply every control on its own. Any other status or message fails closed.
fn classic_protection_absent(stderr: &[u8], stdout: &[u8]) -> bool {
    stderr.windows(10).any(|window| window == b"(HTTP 404)")
        && serde_json::from_slice::<Value>(stdout).is_ok_and(|value| {
            value.get("status").and_then(Value::as_str) == Some("404")
                && matches!(
                    value.get("message").and_then(Value::as_str),
                    Some("Not Found" | "Branch not protected")
                )
        })
}

fn encode_segment(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

/// Require the metadata/tag environment's human approval and self-review guard.
pub(super) fn verify_stable_tag_environment(
    api: &GitHubApi,
    branch: &str,
) -> Result<(), OrchestratorError> {
    verify_stable_environment(&api.repository, branch)
        .map_err(|_| internal("stable_tag_environment_unprotected"))
}
