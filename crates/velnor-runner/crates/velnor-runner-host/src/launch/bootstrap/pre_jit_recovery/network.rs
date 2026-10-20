use std::{
    collections::BTreeMap,
    future::Future,
    time::{Duration, Instant},
};

use bollard::{Docker, errors::Error as DockerError, models::NetworkInspect};
use serde::Serialize;

use crate::{
    docker_client::docker_deadline_after,
    error::HostError,
    journal::{ConnectionJournal, PreJitCleanupClaim, PreJitCleanupInventory},
    launch::{SelectedEngineContext, VerifiedWorkerImages, bootstrap::containers},
    worker::WorkerIdentityNames,
};

use super::bootstrap::verify_selection;
use super::{PreJitCleanupPhase, PreJitCleanupPhaseProof};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn remove_if_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    inventory: &PreJitCleanupInventory,
) -> Result<(), HostError> {
    let identity = inventory.network();
    let plan = inventory.storage_plan();
    if plan.reservation_id() != claim.reservation_id()
        || plan.operation_id() != claim.operation_id()
        || identity.name()
            != format!(
                "velnor-{}-private",
                WorkerIdentityNames::from_private_volume(
                    inventory
                        .initializer()
                        .home_volume()
                        .strip_suffix("-home")
                        .ok_or(HostError::Ownership)?
                )?
                .remote_runner_name()?
            )
        || identity.create_state() != "requested"
            && identity.create_state() != "created"
            && identity.create_state() != "uncertain"
        || identity.remove_state() != "not_requested"
    {
        return Err(HostError::Ownership);
    }
    let expected_labels = expected_network_labels(identity, claim)?;
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    verify_selection(docker, engine, images, claim, deadline).await?;
    let (observed_id, outcome) = remove_network_if_present(
        docker,
        engine,
        images,
        claim,
        identity,
        &expected_labels,
        deadline,
    )
    .await?;
    let selected = verify_selection(docker, engine, images, claim, deadline).await?;
    let evidence = serde_json::to_vec(&NetworkPhaseEvidence {
        engine_id: selected.engine_id,
        docker_root_digest: selected.docker_root_digest,
        network_name: identity.name(),
        saved_id: identity.id(),
        observed_id: observed_id.as_deref(),
        labels: &expected_labels,
        create_state: identity.create_state(),
        outcome,
    })
    .map_err(|_| HostError::Identity)?;
    let proof = PreJitCleanupPhaseProof::after_observed_phase(
        claim,
        PreJitCleanupPhase::StorageAbsent,
        PreJitCleanupPhase::NetworkAbsent,
        &evidence,
    )?;
    journal.record_pre_jit_cleanup_phase(claim, &proof).await
}

fn expected_network_labels(
    identity: &crate::journal::PreJitNetworkIdentity,
    claim: &PreJitCleanupClaim,
) -> Result<BTreeMap<String, String>, HostError> {
    let expected = BTreeMap::from([
        ("io.velnor.managed".to_owned(), "true".to_owned()),
        (
            "io.velnor.connection-id".to_owned(),
            claim.connection_id().to_owned(),
        ),
        (
            "io.velnor.engine-id".to_owned(),
            claim.engine_id().to_owned(),
        ),
        (
            "io.velnor.operation-id".to_owned(),
            claim.operation_id().to_string(),
        ),
        (
            "io.velnor.reservation-id".to_owned(),
            claim.reservation_id().to_string(),
        ),
    ]);
    let persisted: BTreeMap<String, String> =
        serde_json::from_str(identity.labels_json()).map_err(|_| HostError::Journal)?;
    if serde_json::to_string(&persisted).map_err(|_| HostError::Journal)? != identity.labels_json()
        || persisted != expected
    {
        return Err(HostError::Ownership);
    }
    Ok(expected)
}

async fn remove_network_if_present(
    docker: &Docker,
    engine: &SelectedEngineContext,
    images: &VerifiedWorkerImages,
    claim: &PreJitCleanupClaim,
    identity: &crate::journal::PreJitNetworkIdentity,
    expected_labels: &BTreeMap<String, String>,
    deadline: Instant,
) -> Result<(Option<String>, &'static str), HostError> {
    let Some(network) = inspect_network(docker, identity.name(), deadline).await? else {
        if let Some(saved_id) = identity.id()
            && (inspect_network(docker, saved_id, deadline).await?.is_some() || !valid_id(saved_id))
        {
            return Err(HostError::Ownership);
        }
        return Ok((identity.id().map(str::to_owned), "already_absent"));
    };
    let actual_id = network
        .id
        .as_deref()
        .filter(|id| valid_id(id))
        .ok_or(HostError::Ownership)?;
    if identity.id().is_some_and(|saved| saved != actual_id) {
        return Err(HostError::Ownership);
    }
    containers::validate_pre_jit_network(&network, identity.name(), actual_id, expected_labels)?;
    verify_selection(docker, engine, images, claim, deadline).await?;
    request(deadline, docker.remove_network(actual_id))
        .await?
        .map_err(|_| HostError::Docker)?;
    if inspect_network(docker, identity.name(), deadline)
        .await?
        .is_some()
        || inspect_network(docker, actual_id, deadline)
            .await?
            .is_some()
    {
        return Err(HostError::Ownership);
    }
    Ok((Some(actual_id.to_owned()), "removed_and_absent"))
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
        .filter(|duration| !duration.is_zero())
        .ok_or(HostError::Docker)?;
    docker_deadline_after(future, remaining.min(REQUEST_TIMEOUT)).await
}

fn valid_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Serialize)]
struct NetworkPhaseEvidence<'a> {
    engine_id: String,
    docker_root_digest: String,
    network_name: &'a str,
    saved_id: Option<&'a str>,
    observed_id: Option<&'a str>,
    labels: &'a BTreeMap<String, String>,
    create_state: &'a str,
    outcome: &'static str,
}
