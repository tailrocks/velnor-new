// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    REQUEST_TIMEOUT, VerifiedStorageNetworkRemoved, inspect_network, request, validate_network,
    verify_engine,
};
use crate::{
    error::HostError,
    journal::{ConnectionJournal, StorageNetworkRemovalClaim, UnboundRetirementClaim},
    launch::SelectedEngineContext,
};
use bollard::Docker;
use std::time::{Duration, Instant};
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) async fn remove_unbound_v48_for_test(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    retirement: &UnboundRetirementClaim,
) -> Result<(), HostError> {
    journal
        .renew_unbound_retirement_claim_v48_for_test(retirement)
        .await?;
    let reservation_id = retirement.reservation_id();
    let claim = match journal
        .claim_storage_network_removal(reservation_id)
        .await?
    {
        Some(claim) => claim,
        None => match journal
            .recover_storage_network_removal(reservation_id)
            .await?
        {
            Some(claim) => claim,
            None => return Ok(()),
        },
    };
    let deadline = Instant::now() + NETWORK_TIMEOUT;
    let result = remove_claimed(
        docker,
        journal,
        engine,
        &claim,
        RemovalAuthority::Unbound(retirement),
        deadline,
    )
    .await;
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

enum RemovalAuthority<'a> {
    Unbound(&'a UnboundRetirementClaim),
}

async fn remove_claimed(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    claim: &StorageNetworkRemovalClaim,
    authority: RemovalAuthority<'_>,
    deadline: Instant,
) -> Result<VerifiedStorageNetworkRemoved, HostError> {
    if claim.engine_id() != engine.engine_id()
        || claim.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    verify_removal_context(docker, journal, engine, claim, &authority, deadline).await?;
    let target_id = resolve_target_id(docker, claim, deadline).await?;
    remove_and_verify_target(
        docker,
        journal,
        engine,
        claim,
        &authority,
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
    authority: &RemovalAuthority<'_>,
    deadline: Instant,
) -> Result<(), HostError> {
    validate_authority(journal, authority).await?;
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    validate_authority(journal, authority).await?;
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
    authority: &RemovalAuthority<'_>,
    target_id: Option<&str>,
    deadline: Instant,
) -> Result<(), HostError> {
    if let Some(target_id) = target_id {
        validate_authority(journal, authority).await?;
        verify_engine(
            docker,
            engine,
            claim.engine_id(),
            claim.docker_root_digest(),
            deadline,
        )
        .await?;
        validate_authority(journal, authority).await?;
        request(deadline, docker.remove_network(target_id))
            .await?
            .map_err(|_| HostError::Docker)?;
    }
    validate_authority(journal, authority).await?;
    verify_engine(
        docker,
        engine,
        claim.engine_id(),
        claim.docker_root_digest(),
        deadline,
    )
    .await?;
    validate_authority(journal, authority).await?;
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

async fn validate_authority(
    journal: &ConnectionJournal,
    authority: &RemovalAuthority<'_>,
) -> Result<(), HostError> {
    let RemovalAuthority::Unbound(claim) = authority;
    journal
        .validate_unbound_retirement_claim_v48_for_test(claim)
        .await
}
