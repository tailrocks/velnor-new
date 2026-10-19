use std::time::{Duration, Instant};

use bollard::Docker;

use crate::error::HostError;
use crate::journal::{ConnectionJournal, StorageNetworkRemovalClaim};
use crate::launch::SelectedEngineContext;

use super::{
    REQUEST_TIMEOUT, VerifiedStorageNetworkRemoved, inspect_network, request, validate_network,
    verify_engine,
};

const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

/// Remove one recorded network after the journal grants a cleanup claim.
pub(crate) async fn remove_owned_network(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
) -> Result<(), HostError> {
    let Some(claim) = journal
        .claim_storage_network_removal(reservation_id)
        .await?
    else {
        return Err(HostError::Journal);
    };
    let deadline = Instant::now() + NETWORK_TIMEOUT;
    let result = remove_claimed(docker, journal, engine, &claim, deadline).await;
    if result.is_err() {
        let _marked = tokio::time::timeout(
            REQUEST_TIMEOUT,
            journal.mark_storage_network_removal_uncertain(&claim),
        )
        .await;
    }
    result
}

async fn remove_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    claim: &StorageNetworkRemovalClaim,
    deadline: Instant,
) -> Result<(), HostError> {
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    if let Some(inspected) = inspect_network(docker, claim.network_name(), deadline).await? {
        let actual_id = inspected.id.as_deref().ok_or(HostError::Ownership)?;
        validate_network(
            &inspected,
            claim.network_name(),
            claim.network_id().unwrap_or(actual_id),
            claim.labels(),
        )?;
        if claim
            .network_id()
            .is_some_and(|expected| expected != actual_id)
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
        request(deadline, docker.remove_network(claim.network_name()))
            .await?
            .map_err(|_| HostError::Docker)?;
    }
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
    let proof = VerifiedStorageNetworkRemoved {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        network_name: claim.network_name().to_owned(),
        network_id: claim.network_id().map(str::to_owned),
        labels: claim.labels().clone(),
    };
    journal.record_storage_network_removed(claim, &proof).await
}
