//! Runner context checks for normal `OpenTofu` cache evidence.

use serde_json::Value;
use std::env;
use std::path::PathBuf;

const MAX_GITHUB_EVENT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitHubRunContext {
    pub(super) repository: String,
    pub(super) repository_id: u64,
    pub(super) git_ref: String,
    pub(super) source_sha: String,
    pub(super) workflow_ref: String,
    pub(super) workflow_sha: String,
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
}

pub(super) fn read_github_run_context() -> Result<GitHubRunContext, &'static str> {
    if env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true") {
        return Err("normal_tofu_github_context_invalid");
    }
    let repository_raw = required("GITHUB_REPOSITORY")?;
    let repository = crate::origin::validate_repository_slug(&repository_raw)
        .ok_or("normal_tofu_github_context_invalid")?;
    let repository_id = positive_u64(&required("GITHUB_REPOSITORY_ID")?)?;
    let git_ref = required("GITHUB_REF")?;
    let source_sha = git_sha(&required("GITHUB_SHA")?)?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    if workflow_ref
        .split_once('@')
        .is_none_or(|(workflow, reference)| workflow.is_empty() || reference.is_empty())
    {
        return Err("normal_tofu_github_context_invalid");
    }
    let workflow_sha = git_sha(&required("GITHUB_WORKFLOW_SHA")?)?;
    let run_id = positive_u64(&required("GITHUB_RUN_ID")?)?;
    let run_attempt = u32::try_from(positive_u64(&required("GITHUB_RUN_ATTEMPT")?)?)
        .map_err(|_| "normal_tofu_github_context_invalid")?;
    let event_path = PathBuf::from(
        env::var_os("GITHUB_EVENT_PATH").ok_or("normal_tofu_github_context_invalid")?,
    );
    let event_text = crate::safe_read::read_event_file(&event_path, MAX_GITHUB_EVENT_BYTES)
        .map_err(|_| "normal_tofu_github_event_invalid")?;
    validate_event_repository(&event_text, &repository, repository_id)?;
    Ok(GitHubRunContext {
        repository,
        repository_id,
        git_ref,
        source_sha,
        workflow_ref,
        workflow_sha,
        run_id,
        run_attempt,
    })
}

pub(super) fn validate_event_repository(
    event_text: &str,
    repository: &str,
    repository_id: u64,
) -> Result<(), &'static str> {
    let event: Value = velnor_actions_contract::parse_strict_json(event_text)
        .map_err(|_| "normal_tofu_github_event_invalid")?;
    let event_repository = event
        .get("repository")
        .and_then(Value::as_object)
        .ok_or("normal_tofu_github_event_invalid")?;
    let event_id = event_repository
        .get("id")
        .and_then(Value::as_u64)
        .ok_or("normal_tofu_github_event_invalid")?;
    let event_name = event_repository
        .get("full_name")
        .and_then(Value::as_str)
        .and_then(crate::origin::validate_repository_slug)
        .ok_or("normal_tofu_github_event_invalid")?;
    if event_id != repository_id || event_name != repository {
        return Err("normal_tofu_github_repository_mismatch");
    }
    Ok(())
}

fn required(name: &str) -> Result<String, &'static str> {
    let value = env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or("normal_tofu_github_context_invalid")?;
    if value.len() > 4_096 || value.chars().any(char::is_control) {
        return Err("normal_tofu_github_context_invalid");
    }
    Ok(value)
}

pub(super) fn positive_u64(value: &str) -> Result<u64, &'static str> {
    value
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0 && number.to_string() == value)
        .ok_or("normal_tofu_github_context_invalid")
}

pub(super) fn git_sha(value: &str) -> Result<String, &'static str> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("normal_tofu_github_context_invalid");
    }
    Ok(value.to_owned())
}
