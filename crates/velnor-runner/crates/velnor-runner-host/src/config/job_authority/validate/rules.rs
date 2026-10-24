use super::super::{ApprovedJobAuthorityPolicy, ConcurrencyControl, MAX_TEXT_BYTES};
use crate::config::{PullRequestRule, SourcePolicy, SourcePolicyMode, WorkflowEvent};
use crate::error::HostError;

pub(super) fn source_policy_allows_ref(
    policy: &SourcePolicy,
    event: WorkflowEvent,
    event_ref: &str,
) -> bool {
    match event_ref.strip_prefix("refs/heads/") {
        Some(branch)
            if matches!(
                event,
                WorkflowEvent::Push
                    | WorkflowEvent::Schedule
                    | WorkflowEvent::WorkflowDispatch
                    | WorkflowEvent::MergeGroup
            ) =>
        {
            policy.mode == SourcePolicyMode::Repository
                || policy.branches.iter().any(|allowed| allowed == branch)
        }
        _ if event_ref.starts_with("refs/tags/") => {
            policy.mode == SourcePolicyMode::Repository
                || (matches!(event, WorkflowEvent::Push | WorkflowEvent::Release)
                    && policy
                        .tag_prefixes
                        .iter()
                        .any(|prefix| event_ref["refs/tags/".len()..].starts_with(prefix)))
        }
        _ => false,
    }
}

pub(super) fn pr_rule_is_subset(rule: &PullRequestRule, source: &PullRequestRule) -> bool {
    rule.base_repository_id == source.base_repository_id
        && rule.base_branch == source.base_branch
        && rule.head_repository_id == source.head_repository_id
        && rule.head_branch == source.head_branch
        && (source.number.is_none() || rule.number == source.number)
}

pub(super) fn valid_event_ref(event: WorkflowEvent, value: &str) -> bool {
    (match event {
        WorkflowEvent::Push => value.starts_with("refs/heads/") || value.starts_with("refs/tags/"),
        WorkflowEvent::Release => value.starts_with("refs/tags/"),
        WorkflowEvent::WorkflowDispatch => {
            value.starts_with("refs/heads/") || value.starts_with("refs/tags/")
        }
        WorkflowEvent::MergeGroup | WorkflowEvent::Schedule => value.starts_with("refs/heads/"),
        WorkflowEvent::PullRequest => false,
    }) && valid_ref(value)
}

fn valid_ref(value: &str) -> bool {
    value.len() <= MAX_TEXT_BYTES
        && !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('-')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && value != "@"
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
        && value.split('/').all(|part| {
            !part.is_empty() && !part.starts_with('.') && !part.as_bytes().ends_with(b".lock")
        })
}

pub(super) fn valid_workflow_path(value: &str) -> bool {
    let yaml_extension = std::path::Path::new(value)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        });
    value.starts_with(".github/workflows/")
        && value.len() <= MAX_TEXT_BYTES
        && yaml_extension
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
        && !value.contains('\\')
}

pub(super) fn valid_job_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(super) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(super) fn valid_environment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.contains("${{")
        && !value.chars().any(char::is_control)
}

pub(super) fn unique_environments(values: &[String]) -> bool {
    let mut names = std::collections::HashSet::new();
    values
        .iter()
        .all(|value| names.insert(value.to_ascii_lowercase()))
}

pub(super) fn valid_permission(value: &str) -> bool {
    matches!(
        value,
        "actions"
            | "attestations"
            | "checks"
            | "contents"
            | "deployments"
            | "discussions"
            | "id-token"
            | "issues"
            | "models"
            | "packages"
            | "pages"
            | "pull-requests"
            | "repository-projects"
            | "security-events"
            | "statuses"
    )
}

pub(super) fn valid_concurrency(value: &ConcurrencyControl) -> bool {
    !value.group.is_empty()
        && value.group.len() <= MAX_TEXT_BYTES
        && !value.group.chars().any(char::is_control)
        && !value.cancel_in_progress.is_empty()
        && value.cancel_in_progress.len() <= MAX_TEXT_BYTES
        && !value.cancel_in_progress.chars().any(char::is_control)
}

pub(super) fn unique_sorted<T: PartialEq>(values: &[T]) -> bool {
    !values.windows(2).any(|pair| pair[0] == pair[1])
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn is_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn encoded_policy_len(policy: &ApprovedJobAuthorityPolicy) -> Result<usize, HostError> {
    serde_json::to_vec(policy)
        .map(|bytes| bytes.len())
        .map_err(|_| HostError::Config)
}
