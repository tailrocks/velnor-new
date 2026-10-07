//! Structural validation for the host configuration.

use super::{DockerConfig, GithubSection, JobTrustPolicy};
use velnor_runner_journal::HostError;

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
        || has_duplicates(&policy.allowed_events)
        || has_duplicates(&policy.allowed_workflow_paths)
    {
        return Err(HostError::Config);
    }
    Ok(())
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
