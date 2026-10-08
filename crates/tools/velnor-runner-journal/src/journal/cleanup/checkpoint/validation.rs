//! Input validation and canonical field encodings for cleanup checkpoints.

use crate::error::HostError;
use crate::journal::PostActionDisposition;

use super::{CleanupCheckpointIdentity, CleanupChildren, CleanupDiagnostics, CleanupStopPolicy};

pub(super) fn validate_begin(
    identity: &CleanupCheckpointIdentity,
    post_actions: &PostActionDisposition,
    policy: &CleanupStopPolicy,
) -> Result<(), HostError> {
    let event_fields = [
        identity.observed_workflow_run_id.is_some(),
        identity.observed_job_id.is_some(),
        identity.observed_runner_id.is_some(),
        identity.observed_runner_name.is_some(),
    ];
    let rest_fields = [
        identity.observed_attempt.is_some(),
        identity.observed_actions_job_id.is_some(),
    ];
    if identity.launch_id <= 0
        || !safe_token(&identity.expected_runner_name, 64)
        || !safe_token(&identity.worker_volume, 128)
        || !container_id(&identity.runner_container_id)
        || !container_id(&identity.dind_container_id)
        || identity.runner_container_id == identity.dind_container_id
        || (!event_fields.iter().all(|value| *value) && event_fields.iter().any(|value| *value))
        || (!rest_fields.iter().all(|value| *value) && rest_fields.iter().any(|value| *value))
        || identity
            .observed_runner_name
            .as_deref()
            .is_some_and(|name| !safe_token(name, 64) || name != identity.expected_runner_name)
        || identity.observed_job_id.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
        })
        || identity
            .observed_workflow_run_id
            .is_some_and(|value| value <= 0)
        || identity.observed_attempt.is_some_and(|value| value <= 0)
        || identity
            .observed_actions_job_id
            .is_some_and(|value| value <= 0)
        || identity.observed_runner_id.is_some_and(|value| value <= 0)
        || !validate_network_pair(
            identity.outer_network_name.as_deref(),
            identity.outer_network_id.as_deref(),
        )
    {
        return Err(HostError::Journal);
    }
    if let PostActionDisposition::Interrupted { reason_class } = post_actions
        && !valid_reason_class(reason_class)
    {
        return Err(HostError::Journal);
    }
    if let CleanupStopPolicy::StopAtDeadline {
        grace_seconds,
        reason_class: value,
    } = policy
        && (*grace_seconds == 0 || !valid_reason_class(value))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) fn validate_step(value: &str) -> Result<(), HostError> {
    if value.is_empty()
        || value.len() > 512
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) fn validate_children(children: &CleanupChildren) -> Result<(), HostError> {
    if children.containers.len() > 4096
        || children.networks.len() > 4096
        || children
            .containers
            .iter()
            .chain(&children.networks)
            .any(|id| !container_id(id))
        || children
            .containers
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || children.networks.windows(2).any(|pair| pair[0] >= pair[1])
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) fn validate_diagnostics(receipt: &CleanupDiagnostics) -> Result<(), HostError> {
    let not_run_receipt = receipt.relative_path.is_empty()
        && receipt.sha256 == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        && receipt.bytes == 0
        && receipt.source_absent;
    let archive_receipt = receipt.relative_path.starts_with("launch-")
        && receipt.relative_path.ends_with("/runner-diagnostics.tar")
        && receipt.relative_path.len() <= 512
        && receipt
            .relative_path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
        && !receipt.source_absent;
    if !(not_run_receipt || archive_receipt)
        || !valid_digest(&receipt.sha256)
        || !receipt.redacted
        || !receipt.retained
        || receipt.bytes > i64::MAX as u64
    {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) fn validate_network_pair(name: Option<&str>, id: Option<&str>) -> bool {
    match (name, id) {
        (None, None) => true,
        (Some(name), Some(id)) => safe_token(name, 128) && container_id(id),
        _ => false,
    }
}

pub(super) fn policy_fields(policy: &CleanupStopPolicy) -> (String, Option<i64>, Option<String>) {
    match policy {
        CleanupStopPolicy::RequireStopped => ("require_stopped".to_owned(), None, None),
        CleanupStopPolicy::StopAtDeadline {
            grace_seconds,
            reason_class,
        } => (
            "stop_at_deadline".to_owned(),
            Some(i64::from(*grace_seconds)),
            Some(reason_class.clone()),
        ),
    }
}

pub(super) fn post_action_fields(value: &PostActionDisposition) -> (&'static str, Option<String>) {
    match value {
        PostActionDisposition::Completed => ("completed", None),
        PostActionDisposition::NotRun => ("not_run", None),
        PostActionDisposition::Interrupted { reason_class } => {
            ("interrupted", Some(reason_class.clone()))
        }
        PostActionDisposition::Unknown => ("unknown", None),
    }
}

pub(super) fn safe_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(super) fn container_id(value: &str) -> bool {
    (12..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_reason_class(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(super) fn read_flag(value: i64) -> Result<bool, HostError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostError::Journal),
    }
}
