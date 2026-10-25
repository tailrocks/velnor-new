use std::path::Path;

use velnor_resource_probe::ProbeError;

use super::{Challenge, EVENT_LIMIT, SourceRequest, channel};

pub(super) fn request_from_environment(challenge: &Challenge) -> Result<SourceRequest, ProbeError> {
    let repository = required_env("GITHUB_REPOSITORY", 512)?;
    validate_repository(&repository)?;
    let repository_id = required_env("GITHUB_REPOSITORY_ID", 20)?;
    let repository_id = parse_positive_u64(&repository_id, "job_hook_repository_id")?;
    let event = required_env("GITHUB_EVENT_NAME", 64)?;
    let ref_name = required_env("GITHUB_REF", 512)?;
    let tested_sha = required_sha("GITHUB_SHA")?;
    let workflow_ref = required_env("GITHUB_WORKFLOW_REF", 1024)?;
    workflow_path(&workflow_ref, &repository)?;
    let workflow_sha = required_sha("GITHUB_WORKFLOW_SHA")?;
    let run_id = parse_positive_u64(&required_env("GITHUB_RUN_ID", 20)?, "job_hook_run_id")?;
    let run_attempt = required_env("GITHUB_RUN_ATTEMPT", 10)?;
    let run_attempt = u32::try_from(parse_positive_u64(&run_attempt, "job_hook_run_attempt")?)
        .map_err(|_| ProbeError::Invalid("job_hook_run_attempt"))?;
    let job_key = required_env("GITHUB_JOB", 128)?;
    validate_job_key(&job_key)?;

    let base_ref = optional_env("GITHUB_BASE_REF", 512)?;
    let head_ref = optional_env("GITHUB_HEAD_REF", 512)?;
    let (pull_request_number, base_sha, head_sha) =
        if matches!(event.as_str(), "pull_request" | "pull_request_target") {
            let event_path = required_env("GITHUB_EVENT_PATH", 4096)?;
            let payload = channel::read_bounded_path(Path::new(&event_path), EVENT_LIMIT)?;
            let facts = pull_request_facts(&payload)?;
            if base_ref.as_deref() != Some(facts.base_ref.as_str())
                || head_ref.as_deref() != Some(facts.head_ref.as_str())
            {
                return Err(ProbeError::Invalid("job_hook_pull_request_ref"));
            }
            (
                Some(facts.number),
                Some(facts.base_sha),
                Some(facts.head_sha),
            )
        } else {
            (None, None, None)
        };

    Ok(SourceRequest {
        schema_version: 1,
        challenge: challenge.nonce.clone(),
        reservation_id: challenge.reservation_id,
        intent_id: challenge.intent_id,
        repository_id,
        repository_name: repository,
        event_name: event,
        event_ref: ref_name,
        tested_sha,
        run_id,
        run_attempt,
        job_key,
        workflow_ref,
        workflow_sha,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
        pull_request_number,
    })
}

#[derive(Debug)]
pub(super) struct PullRequestFacts {
    pub(super) number: u64,
    pub(super) base_ref: String,
    pub(super) base_sha: String,
    pub(super) head_ref: String,
    pub(super) head_sha: String,
}

pub(super) fn pull_request_facts(payload: &[u8]) -> Result<PullRequestFacts, ProbeError> {
    let value: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| ProbeError::Invalid("job_hook_event"))?;
    let number = value
        .get("number")
        .and_then(serde_json::Value::as_u64)
        .filter(|number| *number > 0)
        .ok_or(ProbeError::Invalid("job_hook_pull_request_number"))?;
    let pull_request = value
        .get("pull_request")
        .ok_or(ProbeError::Invalid("job_hook_pull_request"))?;
    let base = pull_request
        .get("base")
        .ok_or(ProbeError::Invalid("job_hook_pull_request_base"))?;
    let head = pull_request
        .get("head")
        .ok_or(ProbeError::Invalid("job_hook_pull_request_head"))?;
    let base_ref = json_string(base, "ref", 512, "job_hook_pull_request_base_ref")?;
    let head_ref = json_string(head, "ref", 512, "job_hook_pull_request_head_ref")?;
    let base_sha = json_string(base, "sha", 40, "job_hook_pull_request_base_sha")?;
    let head_sha = json_string(head, "sha", 40, "job_hook_pull_request_head_sha")?;
    validate_sha(&base_sha, "job_hook_pull_request_base_sha")?;
    validate_sha(&head_sha, "job_hook_pull_request_head_sha")?;
    Ok(PullRequestFacts {
        number,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
    })
}

fn json_string(
    object: &serde_json::Value,
    key: &str,
    max: usize,
    error: &'static str,
) -> Result<String, ProbeError> {
    let value = object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or(ProbeError::Invalid(error))?;
    bounded_text(value, max, error)
}

pub(super) fn workflow_path(workflow_ref: &str, repository: &str) -> Result<String, ProbeError> {
    let (repository_and_path, ref_value) = workflow_ref
        .rsplit_once('@')
        .ok_or(ProbeError::Invalid("job_hook_workflow_ref"))?;
    let prefix = format!("{repository}/");
    let path = repository_and_path
        .strip_prefix(&prefix)
        .ok_or(ProbeError::Invalid("job_hook_workflow_repository"))?;
    if ref_value.is_empty()
        || !path.starts_with(".github/workflows/")
        || path.len() > 512
        || path.contains("..")
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(ProbeError::Invalid("job_hook_workflow_path"));
    }
    Ok(path.to_owned())
}

fn validate_repository(value: &str) -> Result<(), ProbeError> {
    let Some((owner, repository)) = value.split_once('/') else {
        return Err(ProbeError::Invalid("job_hook_repository"));
    };
    if owner.is_empty()
        || repository.is_empty()
        || repository.contains('/')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(ProbeError::Invalid("job_hook_repository"));
    }
    Ok(())
}

fn required_env(name: &'static str, max: usize) -> Result<String, ProbeError> {
    let value = std::env::var(name).map_err(|_| ProbeError::Invalid(name))?;
    bounded_text(&value, max, name)
}

fn optional_env(name: &'static str, max: usize) -> Result<Option<String>, ProbeError> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => Ok(None),
        Ok(value) => bounded_text(&value, max, name).map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ProbeError::Invalid(name)),
    }
}

pub(super) fn bounded_text(
    value: &str,
    max: usize,
    error: &'static str,
) -> Result<String, ProbeError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(ProbeError::Invalid(error));
    }
    Ok(value.to_owned())
}

fn required_sha(name: &'static str) -> Result<String, ProbeError> {
    let value = required_env(name, 40)?;
    validate_sha(&value, name)?;
    Ok(value)
}

pub(super) fn validate_sha(value: &str, field: &'static str) -> Result<(), ProbeError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProbeError::Invalid(field));
    }
    Ok(())
}

fn validate_job_key(value: &str) -> Result<(), ProbeError> {
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ProbeError::Invalid("job_hook_job_key"));
    }
    Ok(())
}

fn parse_positive_u64(value: &str, field: &'static str) -> Result<u64, ProbeError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ProbeError::Invalid(field));
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or(ProbeError::Invalid(field))
}
