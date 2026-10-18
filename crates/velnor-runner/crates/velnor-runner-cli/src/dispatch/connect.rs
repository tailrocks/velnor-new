use std::io::Read;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use velnor_runner_host::{
    ConnectPlan, ConnectionId, HostConfig, HostError, KeychainReference, PullRequestRule,
    RepositoryConnection, ScaleSetSelector, SourcePolicy, WorkflowEvent, connect_plan,
    import_secret,
};

use super::connect_config::sample_config;
use super::connection_state::{
    admission_lock, config_lock, connect_write_error, connection_binding_snapshot,
    connection_drain_marker, connection_drain_proof, connection_lifecycle_lock,
    connection_reactivation_proof, has_reactivation_proof, persist_host, read_connection_snapshot,
    read_host, same_connection_identity, store_token, write_reactivation_proof,
};
use super::marker::{marker_token, remove_marker_if_matches, remove_marker_path};

const KEYCHAIN_SERVICE: &str = "com.tailrocks.velnor.host";

/// Fields for one `connect` invocation. The token is not a field.
pub(super) struct ConnectRequest<'a> {
    pub(super) state: &'a Path,
    pub(super) connection_id: &'a str,
    pub(super) repo: &'a str,
    pub(super) repository_id: i64,
    pub(super) scale_set: &'a str,
    pub(super) scale_set_labels: Vec<String>,
    pub(super) source_policy_mode: &'a str,
    pub(super) events: Vec<String>,
    pub(super) branches: Vec<String>,
    pub(super) tag_prefixes: Vec<String>,
    pub(super) pull_request_rules: Vec<String>,
    pub(super) platform: &'a str,
    pub(super) max_jobs: Option<u32>,
    pub(super) docker_context: Option<&'a str>,
    pub(super) endpoint: Option<&'a str>,
    pub(super) runner_cpu_millicores: u64,
    pub(super) runner_memory_bytes: u64,
    pub(super) dind_cpu_millicores: u64,
    pub(super) dind_memory_bytes: u64,
    pub(super) operational_memory_reserve_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectError {
    Config,
    Lock,
    Rejected,
    Secret(HostError),
    Lifecycle(HostError),
    Write,
    WriteUncertain,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config => formatter.write_str("invalid config"),
            Self::Lock => formatter.write_str("config is locked"),
            Self::Rejected => formatter.write_str("rejected connection"),
            Self::Secret(error) | Self::Lifecycle(error) => {
                std::fmt::Display::fmt(error, formatter)
            }
            Self::Write => formatter.write_str("write failed"),
            Self::WriteUncertain => formatter.write_str("write state uncertain"),
        }
    }
}

pub(super) fn connect(request: &ConnectRequest<'_>) -> ExitCode {
    let mut stdin = std::io::stdin();
    match connect_with(&mut stdin, KEYCHAIN_SERVICE, request) {
        Ok(()) => {
            println!("Connection saved.");
            ExitCode::SUCCESS
        }
        Err(ConnectError::Rejected) => {
            eprintln!("Connection rejected.");
            ExitCode::from(1)
        }
        Err(ConnectError::Secret(error)) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
        Err(ConnectError::Lifecycle(error)) => {
            eprintln!("Connection lifecycle check failed: {error}");
            ExitCode::from(1)
        }
        Err(ConnectError::Lock) => {
            eprintln!("Config is locked.");
            ExitCode::from(1)
        }
        Err(ConnectError::Config) => {
            eprintln!("Config is invalid.");
            ExitCode::from(1)
        }
        Err(ConnectError::Write) => {
            eprintln!("Config write failed.");
            ExitCode::from(1)
        }
        Err(ConnectError::WriteUncertain) => {
            eprintln!("Configuration update is uncertain. Examine host.toml.");
            ExitCode::from(1)
        }
    }
}

pub(super) fn connect_with<R: Read>(
    input: &mut R,
    service: &str,
    request: &ConnectRequest<'_>,
) -> Result<(), ConnectError> {
    connect_with_store(input, service, request, import_secret)
}

pub(super) fn connect_with_store<R, F>(
    input: &mut R,
    service: &str,
    request: &ConnectRequest<'_>,
    store: F,
) -> Result<(), ConnectError>
where
    R: Read,
    F: FnOnce(&str, &str, &[u8]) -> Result<(), HostError>,
{
    connect_with_store_and_reactivate(input, service, request, store, |state, binding| {
        crate::readiness_probe::reactivate_connection_until(
            state,
            binding,
            Instant::now() + super::DRAIN_WAIT_BUDGET,
        )
    })
}

pub(super) fn connect_with_store_and_reactivate<R, F, G>(
    input: &mut R,
    service: &str,
    request: &ConnectRequest<'_>,
    store: F,
    reactivate: G,
) -> Result<(), ConnectError>
where
    R: Read,
    F: FnOnce(&str, &str, &[u8]) -> Result<(), HostError>,
    G: FnOnce(&Path, &RepositoryConnection) -> Result<bool, HostError>,
{
    let connection_id =
        ConnectionId::parse(request.connection_id).map_err(|_| ConnectError::Config)?;
    let _lifecycle_lock =
        connection_lifecycle_lock(request.state, &connection_id).map_err(|_| ConnectError::Lock)?;
    let _config_lock = config_lock(request.state).map_err(|_| ConnectError::Lock)?;
    let existing = read_host(request.state)?;
    let marker_path = connection_drain_marker(request.state, &connection_id);
    let marker = marker_token(&marker_path).map_err(|_| ConnectError::Rejected)?;
    let connection = requested_connection(
        request,
        service,
        existing.as_ref(),
        &connection_id,
        marker.as_ref(),
    )?;
    let text = candidate_config(existing.as_ref(), request, &connection)?;
    let credential_ref = connection.credential_ref.clone();
    authorize_reactivation(request.state, &connection, marker.as_ref(), reactivate)?;
    store_token(input, &credential_ref, store)?;
    if let Some(token) = marker {
        persist_reconnected(request.state, &text, &connection, &marker_path, &token)
    } else {
        persist_host(request.state, &text)
    }
}

fn requested_connection(
    request: &ConnectRequest<'_>,
    service: &str,
    existing: Option<&HostConfig>,
    connection_id: &ConnectionId,
    marker: Option<&super::marker::MarkerToken>,
) -> Result<RepositoryConnection, ConnectError> {
    let snapshot = if marker.is_some() {
        let snapshot = read_connection_snapshot(request.state, connection_id)?
            .ok_or(ConnectError::Rejected)?;
        if existing
            .and_then(|config| config.connection(connection_id))
            .is_some_and(|configured| configured != &snapshot)
        {
            return Err(ConnectError::Rejected);
        }
        Some(snapshot)
    } else {
        None
    };
    let credential_ref = existing
        .and_then(|config| config.connection(connection_id))
        .map(|connection| connection.credential_ref.clone())
        .or_else(|| {
            snapshot
                .as_ref()
                .map(|connection| connection.credential_ref.clone())
        })
        .map_or_else(
            || {
                KeychainReference::parse(&format!("keychain:{service}/{}", connection_id.as_str()))
                    .map_err(|_| ConnectError::Config)
            },
            Ok,
        )?;
    let connection = connection_from(request, connection_id.clone(), credential_ref)?;
    if snapshot.as_ref().is_some_and(|snapshot| {
        !matches!(same_connection_identity(snapshot, &connection), Ok(true))
    }) {
        return Err(ConnectError::Rejected);
    }
    Ok(connection)
}

fn candidate_config(
    existing: Option<&HostConfig>,
    request: &ConnectRequest<'_>,
    connection: &RepositoryConnection,
) -> Result<String, ConnectError> {
    let requested =
        HostConfig::parse(&sample_config(request, connection)).map_err(|_| ConnectError::Config)?;
    let candidate = match existing {
        None => requested,
        Some(current) => {
            if !same_shared_settings(current, &requested)? {
                return Err(ConnectError::Rejected);
            }
            let mut candidate = current.clone();
            match current.connection(&connection.connection_id) {
                Some(stored) if stored == connection => {}
                Some(_) => return Err(ConnectError::Rejected),
                None => candidate
                    .add_connection(connection.clone())
                    .map_err(|_| ConnectError::Rejected)?,
            }
            candidate
        }
    };
    if connect_plan(existing, &candidate) == ConnectPlan::Rejected {
        return Err(ConnectError::Rejected);
    }
    candidate.to_toml().map_err(|_| ConnectError::Config)
}

fn authorize_reactivation<G>(
    state: &Path,
    connection: &RepositoryConnection,
    marker: Option<&super::marker::MarkerToken>,
    reactivate: G,
) -> Result<(), ConnectError>
where
    G: FnOnce(&Path, &RepositoryConnection) -> Result<bool, HostError>,
{
    let Some(token) = marker else {
        return Ok(());
    };
    if has_reactivation_proof(state, &connection.connection_id, connection, token)? {
        return Ok(());
    }
    if !reactivate(state, connection).map_err(ConnectError::Lifecycle)? {
        return Err(ConnectError::Rejected);
    }
    write_reactivation_proof(state, connection, token).map_err(connect_write_error)
}

fn persist_reconnected(
    state: &Path,
    text: &str,
    connection: &RepositoryConnection,
    marker_path: &Path,
    token: &super::marker::MarkerToken,
) -> Result<(), ConnectError> {
    let _admission_lock = admission_lock(state).map_err(|_| ConnectError::Lock)?;
    if marker_token(marker_path).map_err(|_| ConnectError::Rejected)? != Some(token.clone()) {
        return Err(ConnectError::Rejected);
    }
    persist_host(state, text)?;
    if !remove_marker_if_matches(marker_path, token).map_err(connect_write_error)? {
        return Err(ConnectError::Rejected);
    }
    let snapshot_path = connection_binding_snapshot(state, &connection.connection_id);
    let proof_path = connection_reactivation_proof(state, &connection.connection_id);
    let drain_proof_path = connection_drain_proof(state, &connection.connection_id);
    let _snapshot_cleanup = remove_marker_path(&snapshot_path);
    let _proof_cleanup = remove_marker_path(&proof_path);
    let _drain_proof_cleanup = remove_marker_path(&drain_proof_path);
    Ok(())
}

pub(super) fn connection_from(
    request: &ConnectRequest<'_>,
    connection_id: ConnectionId,
    credential_ref: KeychainReference,
) -> Result<RepositoryConnection, ConnectError> {
    let scale_set = ScaleSetSelector::try_new(request.scale_set, &request.scale_set_labels)
        .map_err(|_| ConnectError::Config)?;
    let events = request
        .events
        .iter()
        .map(|event| parse_event(event))
        .collect::<Result<Vec<_>, _>>()?;
    let pull_request_rules = request
        .pull_request_rules
        .iter()
        .map(|rule| parse_pull_request_rule(rule))
        .collect::<Result<Vec<_>, _>>()?;
    let mode = match request.source_policy_mode {
        "repository" => velnor_runner_host::SourcePolicyMode::Repository,
        "branch-restricted" => velnor_runner_host::SourcePolicyMode::BranchRestricted,
        _ => return Err(ConnectError::Config),
    };
    let source_policy = SourcePolicy::try_new(
        mode,
        request.branches.clone(),
        request.tag_prefixes.clone(),
        events,
        pull_request_rules,
    )
    .map_err(|_| ConnectError::Config)?;
    RepositoryConnection::try_new(
        connection_id,
        request.repository_id,
        request.repo.to_owned(),
        credential_ref,
        scale_set,
        source_policy,
    )
    .map_err(|_| ConnectError::Config)
}

fn parse_event(value: &str) -> Result<WorkflowEvent, ConnectError> {
    match value {
        "pull_request" => Ok(WorkflowEvent::PullRequest),
        "push" => Ok(WorkflowEvent::Push),
        "merge_group" => Ok(WorkflowEvent::MergeGroup),
        "schedule" => Ok(WorkflowEvent::Schedule),
        "workflow_dispatch" => Ok(WorkflowEvent::WorkflowDispatch),
        "release" => Ok(WorkflowEvent::Release),
        _ => Err(ConnectError::Config),
    }
}

fn parse_pull_request_rule(value: &str) -> Result<PullRequestRule, ConnectError> {
    let parts = value.splitn(5, ':').collect::<Vec<_>>();
    if parts.len() != 5 {
        return Err(ConnectError::Config);
    }
    let number = if parts[0] == "*" {
        None
    } else {
        Some(parts[0].parse().map_err(|_| ConnectError::Config)?)
    };
    Ok(PullRequestRule {
        number,
        base_repository_id: parts[1].parse().map_err(|_| ConnectError::Config)?,
        base_branch: parts[2].to_owned(),
        head_repository_id: parts[3].parse().map_err(|_| ConnectError::Config)?,
        head_branch: parts[4].to_owned(),
    })
}

pub(super) fn same_shared_settings(
    current: &HostConfig,
    requested: &HostConfig,
) -> Result<bool, ConnectError> {
    let current_resources = current
        .resource_budget()
        .map_err(|_| ConnectError::Config)?;
    let requested_resources = requested
        .resource_budget()
        .map_err(|_| ConnectError::Config)?;
    Ok(current.host.max_jobs == requested.host.max_jobs
        && current_resources == requested_resources
        && current.docker == requested.docker)
}
