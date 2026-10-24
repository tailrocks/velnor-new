use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use bollard::Docker;
use bollard::models::NetworkCreateRequest;

use crate::error::HostError;
use crate::journal::{ConnectionJournal, StorageNetworkCreateClaim};
use crate::launch::SelectedEngineContext;

use super::{
    REQUEST_TIMEOUT, VerifiedStorageNetworkCreated, inspect_network, request, valid_network_id,
    validate_claim, validate_network, verify_engine,
};

const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

/// Create and inspect one journal-owned private network.
pub(crate) async fn create_owned_network(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> Result<crate::journal::RecordedStorageNetwork, HostError> {
    let Some(claim) = journal.claim_storage_network_create(reservation_id).await? else {
        return Err(HostError::Journal);
    };
    let deadline = Instant::now() + NETWORK_TIMEOUT;
    let result = create_claimed(docker, journal, engine, &claim, deadline).await;
    if result.is_err() {
        let _marked = tokio::time::timeout(
            REQUEST_TIMEOUT,
            journal.mark_storage_network_create_uncertain(&claim),
        )
        .await;
    }
    result
}

async fn create_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    claim: &StorageNetworkCreateClaim,
    deadline: Instant,
) -> Result<crate::journal::RecordedStorageNetwork, HostError> {
    validate_claim(claim, engine)?;
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    if inspect_network(docker, claim.network_name(), deadline)
        .await?
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    let labels = claim.labels().clone();
    let response = request(
        deadline,
        docker.create_network(network_create_request(claim, &labels)),
    )
    .await?
    .map_err(|_| HostError::Docker)?;
    if !response.warning.is_empty() || !valid_network_id(&response.id) {
        return Err(HostError::Docker);
    }
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    let inspected = inspect_network(docker, claim.network_name(), deadline)
        .await?
        .ok_or(HostError::Docker)?;
    validate_network(&inspected, claim.network_name(), &response.id, &labels)?;
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    let proof = VerifiedStorageNetworkCreated {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        network_name: claim.network_name().to_owned(),
        network_id: response.id,
        labels,
    };
    journal.record_storage_network_created(claim, &proof).await
}

fn network_create_request(
    claim: &StorageNetworkCreateClaim,
    labels: &BTreeMap<String, String>,
) -> NetworkCreateRequest {
    NetworkCreateRequest {
        name: claim.network_name().to_owned(),
        driver: Some("bridge".to_owned()),
        scope: Some("local".to_owned()),
        internal: Some(false),
        attachable: Some(false),
        ingress: Some(false),
        enable_ipv4: Some(true),
        enable_ipv6: Some(false),
        labels: Some(
            labels
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
        ..Default::default()
    }
}
