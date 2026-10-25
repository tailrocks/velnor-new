//! Decode bounded hook source values.

use serde::Deserialize;

use super::HookSourceError;

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 512;

/// One repository identity in a pull request event snapshot.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct UntrustedHookRepository {
    pub(super) id: i64,
    pub(super) name: String,
    pub(super) full_name: String,
}

/// One branch record in a pull request event snapshot.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct UntrustedHookBranch {
    pub(super) repository: UntrustedHookRepository,
    #[serde(rename = "ref")]
    pub(super) reference: String,
    pub(super) sha: String,
}

/// Pull request fields copied from the runner event file.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UntrustedHookPullRequest {
    pub(super) number: i64,
    pub(super) base: UntrustedHookBranch,
    pub(super) head: UntrustedHookBranch,
}

/// Hook source values supplied by the runner.
///
/// These values remain untrusted until the host binds them to its exact hook channel and runner.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UntrustedHookSourceInput {
    pub(super) repository_id: i64,
    pub(super) repository: String,
    pub(super) run_id: i64,
    pub(super) run_attempt: i64,
    pub(super) event_name: String,
    pub(super) event_ref: String,
    pub(super) workflow_ref: String,
    pub(super) workflow_sha: String,
    pub(super) tested_sha: String,
    pub(super) job_workflow_ref: String,
    pub(super) workflow_job_id: String,
    #[serde(default)]
    pub(super) pull_request: Option<UntrustedHookPullRequest>,
}

impl UntrustedHookSourceInput {
    /// Decode a bounded hook record with no unknown or duplicate fields.
    ///
    /// # Errors
    ///
    /// Gives an error when input exceeds its byte or field limits.
    pub fn decode(bytes: &[u8]) -> Result<Self, HookSourceError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(HookSourceError::TooLarge);
        }
        let input: Self =
            serde_json::from_slice(bytes).map_err(|_| HookSourceError::InvalidInput)?;
        input.validate()?;
        Ok(input)
    }

    pub(super) fn validate(&self) -> Result<(), HookSourceError> {
        let strings = [
            &self.repository,
            &self.event_name,
            &self.event_ref,
            &self.workflow_ref,
            &self.workflow_sha,
            &self.tested_sha,
            &self.job_workflow_ref,
            &self.workflow_job_id,
        ];
        if self.repository_id <= 0
            || self.run_id <= 0
            || self.run_attempt <= 0
            || strings.iter().any(|value| {
                value.is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0')
            })
            || !canonical_repository(&self.repository)
            || !valid_workflow_job_id(&self.workflow_job_id)
            || !valid_sha(&self.workflow_sha)
            || !valid_sha(&self.tested_sha)
        {
            return Err(HookSourceError::InvalidInput);
        }
        match (self.event_name.as_str(), self.pull_request.as_ref()) {
            ("pull_request", Some(pull))
                if self.event_ref == format!("refs/pull/{}/merge", pull.number) =>
            {
                validate_pull(pull)
            }
            ("push" | "workflow_dispatch" | "schedule" | "merge_group", None)
                if self
                    .event_ref
                    .strip_prefix("refs/heads/")
                    .is_some_and(valid_branch) =>
            {
                Ok(())
            }
            ("push" | "release" | "workflow_dispatch", None)
                if self
                    .event_ref
                    .strip_prefix("refs/tags/")
                    .is_some_and(valid_branch) =>
            {
                Ok(())
            }
            _ => Err(HookSourceError::InvalidInput),
        }
    }
}

fn validate_pull(pull: &UntrustedHookPullRequest) -> Result<(), HookSourceError> {
    let fields = [
        &pull.base.repository.name,
        &pull.base.repository.full_name,
        &pull.base.reference,
        &pull.base.sha,
        &pull.head.repository.name,
        &pull.head.repository.full_name,
        &pull.head.reference,
        &pull.head.sha,
    ];
    if pull.number <= 0
        || pull.base.repository.id <= 0
        || pull.head.repository.id <= 0
        || fields
            .iter()
            .any(|value| value.is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0'))
        || !canonical_name(&pull.base.repository.name)
        || !canonical_repository(&pull.base.repository.full_name)
        || !canonical_name(&pull.head.repository.name)
        || !canonical_repository(&pull.head.repository.full_name)
        || !valid_sha(&pull.base.sha)
        || !valid_sha(&pull.head.sha)
    {
        return Err(HookSourceError::InvalidInput);
    }
    Ok(())
}

fn canonical_repository(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && canonical_name(owner)
        && canonical_name(repository)
        && !repository.contains('/')
}

fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("..")
        && !value.bytes().any(|byte| byte.is_ascii_control())
        && !value.contains(' ')
}

fn valid_workflow_job_id(value: &str) -> bool {
    value.len() <= 100
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn canonical_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(super) fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
