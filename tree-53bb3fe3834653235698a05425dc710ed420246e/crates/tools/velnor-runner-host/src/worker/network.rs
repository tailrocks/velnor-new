//! Generation-scoped outer bridge management for official Linux runner jobs.

use std::collections::HashMap;

use ::bollard::Docker;
use ::bollard::errors::Error as DockerError;
use ::bollard::models::{NetworkCreateRequest, NetworkInspect};

use crate::HostError;
use crate::docker_client::docker_deadline;

use super::worker_labels;

mod cleanup;
pub use cleanup::{
    OuterNetworkCleanupEngine, OuterNetworkCleanupLedger, OuterNetworkRemovalReceipt,
    cleanup_provisioning_network,
};

/// Exact deterministic bridge identity for one worker volume generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerNetworkPlan {
    worker_volume: String,
    name: String,
    labels: Vec<String>,
}

/// A failed network operation that preserves any exact ID returned by Docker.
///
/// A missing ID after a timed-out create leaves the deterministic name
/// unresolved; callers must keep the launch fenced and reconcile by name and
/// ownership labels before retrying or removing anything.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("worker network operation failed")]
pub struct WorkerNetworkFailure {
    error: HostError,
    network_id: Option<String>,
    side_effect_may_have_succeeded: bool,
}

impl WorkerNetworkFailure {
    pub(crate) fn new(
        error: HostError,
        network_id: Option<String>,
        side_effect_may_have_succeeded: bool,
    ) -> Self {
        Self {
            error,
            network_id,
            side_effect_may_have_succeeded,
        }
    }

    /// Sanitized underlying host error.
    #[must_use]
    pub const fn error(&self) -> HostError {
        self.error
    }

    /// Exact 64-character ID returned by Docker, when available.
    #[must_use]
    pub fn network_id(&self) -> Option<&str> {
        self.network_id.as_deref()
    }

    /// Whether a bridge may remain after the failure.
    #[must_use]
    pub const fn side_effect_may_have_succeeded(&self) -> bool {
        self.side_effect_may_have_succeeded
    }
}

impl WorkerNetworkPlan {
    /// Build the only supported per-job bridge plan for a worker volume.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::ForbiddenMount`] when the worker volume name is invalid.
    pub fn for_worker(worker_volume: &str) -> Result<Self, HostError> {
        super::volumes::worker_volume_names(worker_volume)?;
        let name = format!("{worker_volume}-outer");
        let mut labels = worker_labels(worker_volume, "outer-network");
        labels.sort_unstable();
        Ok(Self {
            worker_volume: worker_volume.to_owned(),
            name,
            labels,
        })
    }

    /// Durable worker-volume identity that owns this bridge.
    #[must_use]
    pub fn worker_volume(&self) -> &str {
        &self.worker_volume
    }

    /// Deterministic network name persisted before Docker create.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Exact Velnor ownership labels required on inspect and removal.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Whether `id` is one complete lowercase Docker network identifier.
    #[must_use]
    pub fn accepts_id(id: &str) -> bool {
        valid_network_id(id)
    }
}

/// Find or create the exact job bridge, reconciling an earlier uncertain create by name.
///
/// The caller must persist `plan.name()` before invoking this function and
/// persist the returned ID before creating the `DinD` container. A timeout is
/// returned as uncertain; the caller must retain capacity and retry through
/// name/label reconciliation, not mark the effect absent.
///
/// # Errors
///
/// Returns [`WorkerNetworkFailure`] when Docker fails or the observed network
/// does not match the deterministic bridge contract. An exact create-response
/// ID is retained on post-create inspect failures; otherwise the durable name
/// remains the only reconciliation key.
pub async fn ensure_worker_network(
    docker: &Docker,
    plan: &WorkerNetworkPlan,
) -> Result<String, WorkerNetworkFailure> {
    let existing = inspect_network(docker, plan.name())
        .await
        .map_err(|error| WorkerNetworkFailure::new(error, None, true))?;
    if let Some(existing) = existing {
        return validate_network(plan, None, &existing)
            .map_err(|error| WorkerNetworkFailure::new(error, None, true));
    }

    let created = docker_deadline(docker.create_network(create_request(plan)))
        .await
        .map_err(|error| WorkerNetworkFailure::new(error, None, true))?
        .map_err(|_| WorkerNetworkFailure::new(HostError::Docker, None, true))?;
    if !valid_network_id(&created.id) {
        return Err(WorkerNetworkFailure::new(HostError::Docker, None, true));
    }
    let observed = inspect_network(docker, &created.id)
        .await
        .map_err(|error| WorkerNetworkFailure::new(error, Some(created.id.clone()), true))?
        .ok_or_else(|| {
            WorkerNetworkFailure::new(HostError::Docker, Some(created.id.clone()), true)
        })?;
    validate_network(plan, Some(&created.id), &observed)
        .map_err(|error| WorkerNetworkFailure::new(error, Some(created.id), true))
}

/// Remove only the exact detached bridge owned by this worker generation.
///
/// # Errors
///
/// Returns [`HostError::Identity`] for a different ID or ownership label and
/// [`HostError::Docker`] when the network remains attached or cannot be proven absent.
pub async fn remove_worker_network(
    docker: &Docker,
    plan: &WorkerNetworkPlan,
    network_id: &str,
) -> Result<(), HostError> {
    if !valid_network_id(network_id) {
        return Err(HostError::Identity);
    }
    let Some(observed) = inspect_network(docker, network_id).await? else {
        if inspect_network(docker, plan.name()).await?.is_some() {
            return Err(HostError::Identity);
        }
        return Ok(());
    };
    let _id = validate_network(plan, Some(network_id), &observed)?;
    if observed
        .containers
        .as_ref()
        .is_some_and(|containers| !containers.is_empty())
    {
        return Err(HostError::Docker);
    }

    let removal = docker_deadline(docker.remove_network(network_id)).await?;
    let after_id = inspect_network(docker, network_id).await?;
    let after_name = inspect_network(docker, plan.name()).await?;
    if after_id.is_none() && after_name.is_none() {
        return Ok(());
    }
    removal.map_err(|_| HostError::Docker)?;
    Err(HostError::Docker)
}

fn create_request(plan: &WorkerNetworkPlan) -> NetworkCreateRequest {
    NetworkCreateRequest {
        name: plan.name.clone(),
        driver: Some("bridge".to_owned()),
        scope: Some("local".to_owned()),
        internal: Some(false),
        attachable: Some(false),
        enable_ipv4: Some(true),
        labels: Some(label_map(&plan.labels)),
        ..Default::default()
    }
}

async fn inspect_network(
    docker: &Docker,
    id_or_name: &str,
) -> Result<Option<NetworkInspect>, HostError> {
    match docker_deadline(docker.inspect_network(id_or_name, None)).await? {
        Ok(network) => Ok(Some(network)),
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => Ok(None),
        Err(_) => Err(HostError::Docker),
    }
}

async fn inspect_owned_network(
    docker: &Docker,
    plan: &WorkerNetworkPlan,
) -> Result<Option<String>, HostError> {
    let Some(network) = inspect_network(docker, plan.name()).await? else {
        return Ok(None);
    };
    validate_network(plan, None, &network).map(Some)
}

fn validate_network(
    plan: &WorkerNetworkPlan,
    expected_id: Option<&str>,
    network: &NetworkInspect,
) -> Result<String, HostError> {
    let id = network.id.as_deref().ok_or(HostError::Docker)?;
    let labels = network.labels.as_ref().ok_or(HostError::Docker)?;
    if !valid_network_id(id)
        || expected_id.is_some_and(|expected| expected != id)
        || network.name.as_deref() != Some(plan.name())
        || network.driver.as_deref() != Some("bridge")
        || network.scope.as_deref() != Some("local")
        || network.internal != Some(false)
        || network.attachable != Some(false)
        || labels != &label_map(&plan.labels)
    {
        return Err(HostError::Identity);
    }
    Ok(id.to_owned())
}

fn label_map(labels: &[String]) -> HashMap<String, String> {
    labels
        .iter()
        .filter_map(|label| {
            label
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn valid_network_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ::bollard::models::NetworkInspect;

    use super::{WorkerNetworkPlan, valid_network_id, validate_network};

    fn inspected(plan: &WorkerNetworkPlan) -> NetworkInspect {
        NetworkInspect {
            name: Some(plan.name().to_owned()),
            id: Some("a".repeat(64)),
            scope: Some("local".to_owned()),
            driver: Some("bridge".to_owned()),
            internal: Some(false),
            attachable: Some(false),
            labels: Some(
                plan.labels()
                    .iter()
                    .filter_map(|label| {
                        label
                            .split_once('=')
                            .map(|(key, value)| (key.to_owned(), value.to_owned()))
                    })
                    .collect::<HashMap<_, _>>(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn bridge_identity_is_deterministic_and_worker_scoped() -> Result<(), crate::HostError> {
        let first = WorkerNetworkPlan::for_worker("w0123456789abcdef0123456789abcdef")?;
        let same = WorkerNetworkPlan::for_worker("w0123456789abcdef0123456789abcdef")?;
        let other = WorkerNetworkPlan::for_worker("w1123456789abcdef0123456789abcdef")?;

        assert_eq!(first, same);
        assert_ne!(first.name(), other.name());
        assert_eq!(first.name(), "w0123456789abcdef0123456789abcdef-outer");
        assert!(
            first
                .labels()
                .contains(&"velnor.role=outer-network".to_owned())
        );
        assert!(
            first
                .labels()
                .contains(&format!("velnor.worker={}", first.worker_volume()))
        );
        Ok(())
    }

    #[test]
    fn exact_bridge_contract_accepts_only_owned_local_bridges() -> Result<(), crate::HostError> {
        let plan = WorkerNetworkPlan::for_worker("w0123456789abcdef0123456789abcdef")?;
        let network = inspected(&plan);

        let expected_id = "a".repeat(64);
        assert_eq!(
            validate_network(&plan, Some(&expected_id), &network).as_deref(),
            Ok(expected_id.as_str())
        );
        assert!(validate_network(&plan, Some(&"b".repeat(64)), &network).is_err());

        let mut wrong = network.clone();
        wrong.name = Some("another-generation".to_owned());
        assert!(validate_network(&plan, None, &wrong).is_err());

        let mut wrong = network.clone();
        wrong.driver = Some("host".to_owned());
        assert!(validate_network(&plan, None, &wrong).is_err());

        let mut wrong = network.clone();
        wrong.internal = Some(true);
        assert!(validate_network(&plan, None, &wrong).is_err());

        let mut wrong = network.clone();
        wrong.attachable = Some(true);
        assert!(validate_network(&plan, None, &wrong).is_err());

        let mut wrong = network;
        wrong
            .labels
            .as_mut()
            .expect("test labels")
            .insert("velnor.worker".to_owned(), "other-generation".to_owned());
        assert!(validate_network(&plan, None, &wrong).is_err());
        Ok(())
    }

    #[test]
    fn bridge_id_requires_full_lowercase_sha256_hex() {
        assert!(valid_network_id(&"a".repeat(64)));
        assert!(!valid_network_id(&"a".repeat(63)));
        assert!(!valid_network_id(&"A".repeat(64)));
        assert!(!valid_network_id(&format!("{}g", "a".repeat(63))));
    }
}
