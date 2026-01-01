//! Structural validation for the host configuration.

use super::{DockerConfig, GithubSection, JobTrustPolicy, JobTrustRule, ReusableWorkflowRule};
use velnor_runner_journal::HostError;

pub(super) fn linux_drain_timeout_is_valid(seconds: Option<u64>) -> bool {
    seconds.is_some_and(|seconds| (1..=crate::MAX_LINUX_DRAIN_TIMEOUT_SECS).contains(&seconds))
}

pub(super) fn validate_github(github: &GithubSection) -> Result<(), HostError> {
    let scope_ok = match (
        github.registration_scope,
        github.registration_scope_name.as_deref(),
    ) {
        (None | Some(super::RegistrationScopeKind::Repository), None) => true,
        (Some(super::RegistrationScopeKind::Organization), Some(organization)) => {
            safe_segment(organization)
                && split_repository(&github.repository)
                    .is_ok_and(|(owner, _)| owner == organization)
        }
        _ => false,
    };
    if repository_ok(&github.repository)
        && scale_set_name_ok(&github.scale_set_name)
        && credential_ref_ok(&github.credential_ref)
        && scope_ok
    {
        Ok(())
    } else {
        Err(HostError::Config)
    }
}

pub(super) fn validate_trust(
    policy: &JobTrustPolicy,
    config_repository: &str,
) -> Result<(), HostError> {
    if policy.allowed_repositories.len() != 1
        || policy.allowed_repositories[0] != config_repository
        || policy.allowed_events.is_empty()
        || policy.allowed_workflow_paths.is_empty()
        || policy.allow_forks
        || policy
            .allowed_repositories
            .iter()
            .any(|name| !repository_ok(name))
        || policy
            .allowed_events
            .iter()
            .any(|event| !event_name_ok(event))
        || policy
            .allowed_workflow_paths
            .iter()
            .any(|path| !workflow_path_ok(path))
        || policy
            .allowed_head_branches
            .iter()
            .any(|branch| !branch_name_ok(branch))
        || policy
            .workflow_rules
            .iter()
            .any(|rule| !workflow_rule_ok(rule, policy, config_repository))
        || policy
            .allowed_group_workflows
            .iter()
            .any(|workflow| !group_workflow_ok(workflow, config_repository))
        || has_duplicates(&policy.allowed_events)
        || has_duplicates(&policy.allowed_workflow_paths)
        || has_duplicates(&policy.allowed_head_branches)
        || has_duplicates(&policy.allowed_group_workflows)
        || has_duplicate_rules(&policy.workflow_rules)
    {
        return Err(HostError::Config);
    }
    Ok(())
}

fn workflow_rule_ok(rule: &JobTrustRule, policy: &JobTrustPolicy, repository: &str) -> bool {
    let expected_workflow_ref = format!("{repository}/{}", rule.workflow_path);
    rule.workflow_ref == expected_workflow_ref
        && group_workflow_ok(&rule.workflow_ref, repository)
        && workflow_identity_ok(&rule.job_workflow_ref)
        && event_name_ok(&rule.event)
        && (policy.allowed_head_branches.is_empty()
            || policy
                .allowed_head_branches
                .iter()
                .any(|branch| branch == &rule.head_branch))
        && policy
            .allowed_events
            .iter()
            .any(|event| event == &rule.event)
        && policy
            .allowed_workflow_paths
            .iter()
            .any(|path| rest_path_matches_bare_workflow(path, &rule.workflow_path))
        && branch_name_ok(&rule.head_branch)
        && rule.referenced_workflows.iter().all(reusable_workflow_ok)
}

fn reusable_workflow_ok(workflow: &ReusableWorkflowRule) -> bool {
    workflow_identity_ok(&workflow.path)
        && branch_name_ok(&workflow.git_ref)
        && ((workflow.sha.len() == 40 || workflow.sha.len() == 64)
            && workflow.sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn workflow_identity_ok(identity: &str) -> bool {
    let mut parts = identity.splitn(3, '/');
    let (Some(owner), Some(repository), Some(_)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let repository_name = format!("{owner}/{repository}");
    repository_ok(&repository_name) && group_workflow_ok(identity, &repository_name)
}

fn branch_name_ok(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && !value.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\' | b']')
        })
}

fn rest_path_matches_bare_workflow(allowed: &str, rest_path: &str) -> bool {
    rest_path
        .strip_prefix(allowed)
        .and_then(|suffix| suffix.strip_prefix('@'))
        .is_some_and(|reference| !reference.is_empty())
}

pub(super) fn split_repository(repository: &str) -> Result<(&str, &str), HostError> {
    if !repository_ok(repository) {
        return Err(HostError::Config);
    }
    let (owner, name) = repository.split_once('/').ok_or(HostError::Config)?;
    Ok((owner, name))
}

pub(super) fn credential_ref_ok(value: &str) -> bool {
    keychain_ref(value) || value == "systemd-credential:github-token"
}

pub(super) fn keychain_ref(value: &str) -> bool {
    value
        .strip_prefix("keychain:")
        .is_some_and(|name| !name.is_empty() && !name.chars().any(char::is_whitespace))
}

pub(super) fn validate_docker(docker: &DockerConfig) -> Result<(), HostError> {
    if docker.platform == "linux/amd64"
        && !docker.context.is_empty()
        && unix_endpoint(&docker.endpoint)
    {
        Ok(())
    } else {
        Err(HostError::Config)
    }
}

fn has_duplicates(values: &[String]) -> bool {
    let mut unique = std::collections::BTreeSet::new();
    values.iter().any(|value| !unique.insert(value))
}

fn has_duplicate_rules(values: &[JobTrustRule]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

fn workflow_path_ok(path: &str) -> bool {
    path.strip_prefix(".github/workflows/")
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && !path.contains('\\')
                && path
                    .split('/')
                    .all(|segment| segment != "." && segment != ".." && safe_segment(segment))
        })
}

fn group_workflow_ok(workflow: &str, repository: &str) -> bool {
    let Some(identity) = workflow
        .strip_prefix(repository)
        .and_then(|value| value.strip_prefix('/'))
    else {
        return false;
    };
    let Some((file_path, reference)) = identity.split_once('@') else {
        return false;
    };
    if reference.is_empty()
        || reference.contains('@')
        || reference.starts_with('/')
        || reference.ends_with('/')
        || reference.ends_with('.')
        || reference.contains("..")
        || reference.contains("//")
        || reference.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\' | b']')
        })
    {
        return false;
    }
    let Some(workflow_file) = file_path.strip_prefix(".github/workflows/") else {
        return false;
    };
    if workflow_file.is_empty()
        || workflow_file
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        || workflow_file.bytes().any(|byte| {
            byte.is_ascii_control()
                || byte.is_ascii_whitespace()
                || matches!(byte, b'?' | b'*' | b'[' | b'\\' | b']' | b'@')
        })
    {
        return false;
    }
    workflow_file
        .rsplit('/')
        .next()
        .and_then(|file_name| {
            file_name
                .strip_suffix(".yaml")
                .or_else(|| file_name.strip_suffix(".yml"))
        })
        .is_some_and(|stem| !stem.is_empty())
}

fn event_name_ok(event: &str) -> bool {
    !event.is_empty()
        && event
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn repository_ok(repository: &str) -> bool {
    let mut parts = repository.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) => safe_segment(owner) && safe_segment(name),
        _ => false,
    }
}

fn safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn scale_set_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn unix_endpoint(endpoint: &str) -> bool {
    endpoint.strip_prefix("unix://").is_some_and(|path| {
        path.starts_with('/')
            && path.len() > 1
            && !path.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    })
}
