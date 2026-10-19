use std::collections::BTreeMap;
use std::future::Future;
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::errors::Error as DockerError;
use bollard::models::NetworkInspect;

use crate::docker_client::docker_deadline_after;
use crate::error::HostError;
use crate::journal::{EngineContext, StorageNetworkCreateClaim};
use crate::launch::SelectedEngineContext;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

mod create;
mod remove;

pub(crate) use create::create_owned_network;
#[cfg(test)]
pub(crate) use remove::remove_owned_network;

/// Inspected identity of one private network created for a reservation.
pub(crate) struct VerifiedStorageNetworkCreated {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    network_name: String,
    network_id: String,
    labels: BTreeMap<String, String>,
}

impl VerifiedStorageNetworkCreated {
    #[cfg(test)]
    pub(crate) fn test_created(
        claim: &StorageNetworkCreateClaim,
        network: &NetworkInspect,
    ) -> Result<Self, HostError> {
        let network_id = network.id.as_deref().ok_or(HostError::Identity)?;
        validate_network(network, claim.network_name(), network_id, claim.labels())?;
        if !valid_network_id(network_id) {
            return Err(HostError::Identity);
        }
        Ok(Self {
            reservation_id: claim.reservation_id(),
            operation_id: claim.operation_id(),
            engine_id: claim.engine_id().to_owned(),
            docker_root_digest: claim.docker_root_digest().to_owned(),
            network_name: claim.network_name().to_owned(),
            network_id: network_id.to_owned(),
            labels: claim.labels().clone(),
        })
    }

    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }

    pub(crate) fn network_id(&self) -> &str {
        &self.network_id
    }

    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}

/// Inspected absence of one network after an authorized removal request.
pub(crate) struct VerifiedStorageNetworkRemoved {
    reservation_id: i64,
    operation_id: i64,
    engine_id: String,
    docker_root_digest: String,
    network_name: String,
    network_id: Option<String>,
    labels: BTreeMap<String, String>,
}

impl VerifiedStorageNetworkRemoved {
    pub(crate) const fn reservation_id(&self) -> i64 {
        self.reservation_id
    }

    pub(crate) const fn operation_id(&self) -> i64 {
        self.operation_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn docker_root_digest(&self) -> &str {
        &self.docker_root_digest
    }

    pub(crate) fn network_name(&self) -> &str {
        &self.network_name
    }

    pub(crate) fn network_id(&self) -> Option<&str> {
        self.network_id.as_deref()
    }

    pub(crate) fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }
}

fn validate_claim(
    claim: &StorageNetworkCreateClaim,
    engine: &SelectedEngineContext,
) -> Result<(), HostError> {
    if claim.reservation_id() <= 0
        || claim.operation_id() <= 0
        || claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
        || !valid_network_name(claim.network_name())
        || !valid_labels(claim.labels())
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn validate_network(
    network: &NetworkInspect,
    expected_name: &str,
    expected_id: &str,
    expected_labels: &BTreeMap<String, String>,
) -> Result<(), HostError> {
    let labels: BTreeMap<_, _> = network
        .labels
        .as_ref()
        .ok_or(HostError::Ownership)?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if network.name.as_deref() != Some(expected_name)
        || network.id.as_deref() != Some(expected_id)
        || network.driver.as_deref() != Some("bridge")
        || network.scope.as_deref() != Some("local")
        || network.internal != Some(false)
        || network.attachable != Some(false)
        || network.ingress != Some(false)
        || network.enable_ipv4 != Some(true)
        || network.enable_ipv6 != Some(false)
        || network
            .containers
            .as_ref()
            .is_some_and(|items| !items.is_empty())
        || labels != *expected_labels
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn verify_engine(
    docker: &Docker,
    engine: &SelectedEngineContext,
    expected_id: &str,
    expected_root: &str,
    deadline: Instant,
) -> Result<(), HostError> {
    let info = request(deadline, docker.info())
        .await?
        .map_err(|_| HostError::Docker)?;
    let selected = EngineContext {
        engine_id: engine.engine_id().to_owned(),
        endpoint: engine.endpoint().to_owned(),
        platform: engine.platform().to_owned(),
    };
    let current = SelectedEngineContext::capture(&selected, &info)?;
    if current.engine_id() == expected_id && current.docker_root_digest() == expected_root {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}

async fn inspect_network(
    docker: &Docker,
    name: &str,
    deadline: Instant,
) -> Result<Option<NetworkInspect>, HostError> {
    match request(deadline, docker.inspect_network(name, None)).await? {
        Ok(network) => Ok(Some(network)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

async fn request<F: Future>(deadline: Instant, future: F) -> Result<F::Output, HostError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or(HostError::Docker)?;
    docker_deadline_after(future, remaining.min(REQUEST_TIMEOUT)).await
}

fn valid_network_name(value: &str) -> bool {
    value.len() == 48
        && value.starts_with("velnor-v")
        && value.ends_with("-private")
        && value[8..40]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_network_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_labels(labels: &BTreeMap<String, String>) -> bool {
    labels.len() == 5
        && labels
            .get("io.velnor.managed")
            .is_some_and(|value| value == "true")
        && labels.contains_key("io.velnor.connection-id")
        && labels.contains_key("io.velnor.engine-id")
        && labels.contains_key("io.velnor.operation-id")
        && labels.contains_key("io.velnor.reservation-id")
}

#[cfg(test)]
#[path = "network/tests.rs"]
mod tests;
