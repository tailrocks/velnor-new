//! Verify each owned physical runner and its repository registration.

use std::time::{Duration, Instant};

use bollard::{Docker, models::Volume};
use velnor_runner_github::{
    AdminConnectionCall, RegistrationScope, RegistrationTokenCall, Transport, admin_connection,
    get_runner_by_name, registration_token,
    workflow_run::{RepositoryTarget, get_repository_scope},
};

use crate::{
    config::RepositoryConnection,
    docker_client::docker_deadline,
    error::HostError,
    journal::{StorageCleanupIdentity, StorageRole},
};

pub(super) type RemoteRunnerAudit = dyn Fn(
        &RepositoryConnection,
        Option<&velnor_runner_github::RunnerReference>,
        i64,
    ) -> Result<(), HostError>
    + Send
    + Sync;

#[path = "audit/inventory.rs"]
mod inventory;
pub(super) use inventory::audit_inventory_with_remote;

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;

pub(super) fn audit_remote_runner(
    binding: &RepositoryConnection,
    expected: Option<&velnor_runner_github::RunnerReference>,
    scale_set_id: i64,
) -> Result<(), HostError> {
    let expected = expected.ok_or(HostError::Ownership)?;
    if expected.runner_scale_set_id != scale_set_id {
        return Err(HostError::Ownership);
    }
    let credential = crate::keychain::load_secret(
        binding.credential_ref.service(),
        binding.credential_ref.account(),
    )?;
    let pat = crate::host_controller::connection_pat(&credential)?;
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(15))
        .ok_or(HostError::Endpoint)?;
    let mut transport =
        crate::https::HttpsTransport::with_deadline(crate::listen::GITHUB_API, deadline)?;
    audit_remote_runner_with_transport(binding, expected, scale_set_id, pat, &mut transport)
}

trait ReadinessTransport: Transport {
    fn set_origin(&mut self, base: &str) -> Result<(), HostError>;
}

impl ReadinessTransport for crate::https::HttpsTransport {
    fn set_origin(&mut self, base: &str) -> Result<(), HostError> {
        self.set_base(base)
    }
}

fn audit_remote_runner_with_transport<T: ReadinessTransport>(
    binding: &RepositoryConnection,
    expected: &velnor_runner_github::RunnerReference,
    scale_set_id: i64,
    pat: &str,
    transport: &mut T,
) -> Result<(), HostError> {
    if expected.runner_scale_set_id != scale_set_id {
        return Err(HostError::Ownership);
    }
    let (owner, repository) = binding
        .repository
        .split_once('/')
        .ok_or(HostError::Config)?;
    let repository_scope = get_repository_scope(
        transport,
        RepositoryTarget {
            id: binding.repository_id,
            owner,
            name: repository,
        },
        pat,
    )
    .map_err(|_| HostError::Endpoint)?;
    if repository_scope.id() != binding.repository_id
        || repository_scope.owner() != owner
        || repository_scope.name() != repository
    {
        return Err(HostError::Ownership);
    }
    let registration = registration_token(
        transport,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner,
                repo: repository,
            },
            pat,
        },
    )
    .map_err(|_| HostError::Endpoint)?;
    let config_url = format!("https://github.com/{owner}/{repository}");
    let admin = admin_connection(
        transport,
        &AdminConnectionCall {
            config_url: &config_url,
            registration_token: registration.expose(),
        },
    )
    .map_err(|_| HostError::Endpoint)?;
    transport.set_origin(admin.expose_url())?;
    let found = get_runner_by_name(transport, &expected.name, admin.expose_token())
        .map_err(|_| HostError::Endpoint)?
        .ok_or(HostError::Ownership)?;
    if found != *expected {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn inspect_owned_container(
    docker: &Docker,
    id: &str,
    name: &str,
    role: &str,
    identity: &StorageCleanupIdentity,
    connection_id: &str,
) -> Result<(), HostError> {
    #[cfg(test)]
    let error_to_host = |error| {
        trace_docker_failure("inspect_container", role, &error);
        HostError::Docker
    };
    #[cfg(not(test))]
    let error_to_host = |_| HostError::Docker;
    let inspected = docker_deadline(docker.inspect_container(id, None))
        .await?
        .map_err(error_to_host)?;
    let labels = inspected
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .ok_or(HostError::Ownership)?;
    let expected = std::collections::HashMap::from([
        ("io.velnor.managed", "true".to_owned()),
        (
            "io.velnor.reservation-id",
            identity.reservation_id().to_string(),
        ),
        (
            "io.velnor.operation-id",
            identity.operation_id().to_string(),
        ),
        ("io.velnor.connection-id", connection_id.to_owned()),
        ("io.velnor.role", role.to_owned()),
    ]);
    if inspected.id.as_deref() != Some(id)
        || inspected.name.as_deref() != Some(format!("/{name}").as_str())
        || expected.len() != labels.len()
        || expected
            .iter()
            .any(|(key, value)| labels.get(*key) != Some(value))
        || inspected.state.as_ref().and_then(|state| state.running) != Some(true)
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn inspect_owned_volume(
    docker: &Docker,
    name: &str,
    role: StorageRole,
    identity: &StorageCleanupIdentity,
    connection_id: &str,
) -> Result<(), HostError> {
    #[cfg(test)]
    let error_to_host = |error| {
        trace_docker_failure("inspect_volume", role.as_str(), &error);
        HostError::Docker
    };
    #[cfg(not(test))]
    let error_to_host = |_| HostError::Docker;
    let volume: Volume = docker_deadline(docker.inspect_volume(name))
        .await?
        .map_err(error_to_host)?;
    let expected = std::collections::HashMap::from([
        ("velnor.connection", connection_id.to_owned()),
        ("velnor.reservation", identity.reservation_id().to_string()),
        ("velnor.operation", identity.operation_id().to_string()),
        ("velnor.role", role.as_str().to_owned()),
    ]);
    if volume.name != name
        || volume.driver != "local"
        || volume.labels.len() != expected.len()
        || expected
            .iter()
            .any(|(key, value)| volume.labels.get(*key) != Some(value))
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn trace_docker_failure(operation: &str, role: &str, error: &bollard::errors::Error) {
    use bollard::errors::Error;

    let detail = match error {
        Error::DockerResponseServerError { status_code, .. } => {
            format!("DockerResponseServerError status={status_code}")
        }
        Error::JsonDataError { column, .. } => format!("JsonDataError column={column}"),
        Error::JsonSerdeError { err } => format!(
            "JsonSerdeError class={:?} line={} column={}",
            err.classify(),
            err.line(),
            err.column()
        ),
        Error::IOError { err } => format!(
            "IOError kind={:?} raw_os_error={:?}",
            err.kind(),
            err.raw_os_error()
        ),
        Error::HyperResponseError { err } => format!(
            "HyperResponseError parse={} incomplete={} closed={} timeout={}",
            err.is_parse(),
            err.is_incomplete_message(),
            err.is_closed(),
            err.is_timeout()
        ),
        Error::HyperLegacyError { err } => {
            format!("HyperLegacyError connect={}", err.is_connect())
        }
        Error::RequestTimeoutError => "RequestTimeoutError".to_owned(),
        other => format!("OtherBollardVariant {:?}", std::mem::discriminant(other)),
    };
    eprintln!("readiness test Docker operation={operation} role={role} error={detail}");
}
