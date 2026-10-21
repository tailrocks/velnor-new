use std::time::{Duration, Instant};

use bollard::Docker;

use crate::error::HostError;
use crate::journal::{ConnectionJournal, PostJitRetirementClaim, StorageNetworkRemovalClaim};
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
    let result = remove_claimed(docker, journal, engine, &claim, None, deadline).await;
    match result {
        Ok(proof) => journal.record_storage_network_removed(&claim, &proof).await,
        Err(error) => {
            let _marked = tokio::time::timeout(
                REQUEST_TIMEOUT,
                journal.mark_storage_network_removal_uncertain(&claim),
            )
            .await;
            Err(error)
        }
    }
}

/// Remove one recorded network while a post-JIT claim authorizes cleanup.
pub(crate) async fn remove_owned_network_for_retirement(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
    retirement: &PostJitRetirementClaim,
) -> Result<(), HostError> {
    if retirement.reservation_id() != reservation_id {
        return Err(HostError::Identity);
    }
    journal
        .validate_post_jit_retirement_claim(retirement)
        .await?;
    let claim = match journal
        .claim_storage_network_removal(reservation_id)
        .await?
    {
        Some(claim) => claim,
        None => journal
            .recover_storage_network_removal(reservation_id)
            .await?
            .ok_or(HostError::Journal)?,
    };
    let deadline = Instant::now() + NETWORK_TIMEOUT;
    let result = remove_claimed(docker, journal, engine, &claim, Some(retirement), deadline).await;
    match result {
        Ok(proof) => {
            journal
                .record_storage_network_removed_for_retirement(retirement, &claim, &proof)
                .await
        }
        Err(error) => {
            let _marked = tokio::time::timeout(
                REQUEST_TIMEOUT,
                journal.mark_storage_network_removal_uncertain(&claim),
            )
            .await;
            Err(error)
        }
    }
}

async fn remove_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    claim: &StorageNetworkRemovalClaim,
    retirement: Option<&PostJitRetirementClaim>,
    deadline: Instant,
) -> Result<VerifiedStorageNetworkRemoved, HostError> {
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    verify_removal_context(docker, journal, engine, claim, retirement, deadline).await?;
    let target_id = resolve_target_id(docker, claim, deadline).await?;
    remove_and_verify_target(
        docker,
        journal,
        engine,
        claim,
        retirement,
        target_id.as_deref(),
        deadline,
    )
    .await?;

    let proof = VerifiedStorageNetworkRemoved {
        reservation_id: claim.reservation_id(),
        operation_id: claim.operation_id(),
        engine_id: claim.engine_id().to_owned(),
        docker_root_digest: claim.docker_root_digest().to_owned(),
        network_name: claim.network_name().to_owned(),
        network_id: claim.network_id().map(str::to_owned),
        labels: claim.labels().clone(),
    };
    Ok(proof)
}

async fn verify_removal_context(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    claim: &StorageNetworkRemovalClaim,
    retirement: Option<&PostJitRetirementClaim>,
    deadline: Instant,
) -> Result<(), HostError> {
    validate_retirement(journal, retirement).await?;
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    validate_retirement(journal, retirement).await?;
    Ok(())
}

async fn resolve_target_id(
    docker: &Docker,
    claim: &StorageNetworkRemovalClaim,
    deadline: Instant,
) -> Result<Option<String>, HostError> {
    let by_name = inspect_network(docker, claim.network_name(), deadline).await?;
    if let Some(network) = by_name.as_ref() {
        let actual_id = network
            .id
            .as_deref()
            .filter(|id| super::valid_network_id(id))
            .ok_or(HostError::Ownership)?;
        if claim
            .network_id()
            .is_some_and(|saved_id| saved_id != actual_id)
        {
            return Err(HostError::Ownership);
        }
        validate_network(network, claim.network_name(), actual_id, claim.labels())?;
    }

    let target_id = match claim.network_id() {
        Some(saved_id) => {
            if !super::valid_network_id(saved_id) {
                return Err(HostError::Ownership);
            }
            match inspect_network(docker, saved_id, deadline).await? {
                Some(network) => {
                    validate_network(&network, claim.network_name(), saved_id, claim.labels())?;
                    Some(saved_id.to_owned())
                }
                None if by_name.is_none() => None,
                None => return Err(HostError::Ownership),
            }
        }
        None => by_name
            .as_ref()
            .and_then(|network| network.id.as_deref())
            .map(str::to_owned),
    };
    Ok(target_id)
}

async fn remove_and_verify_target(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    claim: &StorageNetworkRemovalClaim,
    retirement: Option<&PostJitRetirementClaim>,
    target_id: Option<&str>,
    deadline: Instant,
) -> Result<(), HostError> {
    if let Some(target_id) = target_id {
        validate_retirement(journal, retirement).await?;
        verify_engine(
            docker,
            engine,
            claim.engine_id(),
            claim.docker_root_digest(),
            deadline,
        )
        .await?;
        validate_retirement(journal, retirement).await?;
        request(deadline, docker.remove_network(target_id))
            .await?
            .map_err(|_| HostError::Docker)?;
    }
    validate_retirement(journal, retirement).await?;
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    validate_retirement(journal, retirement).await?;
    if let Some(known_id) = claim.network_id().or(target_id)
        && inspect_network(docker, known_id, deadline).await?.is_some()
    {
        return Err(HostError::Ownership);
    }
    if inspect_network(docker, claim.network_name(), deadline)
        .await?
        .is_some()
    {
        return Err(HostError::Ownership);
    }
    Ok(())
}

async fn validate_retirement(
    journal: &ConnectionJournal,
    retirement: Option<&PostJitRetirementClaim>,
) -> Result<(), HostError> {
    if let Some(retirement) = retirement {
        journal
            .validate_post_jit_retirement_claim(retirement)
            .await?;
    }
    Ok(())
}
