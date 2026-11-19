// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use bollard::Docker;

use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, JitConflictRecoveryClaim, PostJitRetirementClaim,
    StorageNetworkRemovalClaim, UnboundResourceKind, UnboundRetirementClaim,
};
use crate::launch::SelectedEngineContext;

use super::{REQUEST_TIMEOUT, VerifiedStorageNetworkRemoved, verify_engine};

mod target;

use target::{remove_and_verify_target, resolve_target_id};

const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

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
    let result = remove_claimed(
        docker,
        journal,
        engine,
        &claim,
        RemovalAuthority::None,
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
    let result = remove_claimed(
        docker,
        journal,
        engine,
        &claim,
        RemovalAuthority::Retirement(retirement),
        deadline,
    )
    .await;
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

pub(crate) async fn remove_owned_network_for_jit_conflict(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    reservation_id: i64,
    recovery: &JitConflictRecoveryClaim,
) -> Result<(), HostError> {
    if recovery.reservation_id() != reservation_id
        || recovery.engine_id() != engine.engine_id()
        || recovery.docker_root_digest() != engine.docker_root_digest()
    {
        return Err(HostError::Identity);
    }
    journal.validate_jit_conflict_claim(recovery).await?;
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
    let result = remove_claimed(
        docker,
        journal,
        engine,
        &claim,
        RemovalAuthority::Conflict(recovery),
        deadline,
    )
    .await;
    match result {
        Ok(proof) => {
            journal.validate_jit_conflict_claim(recovery).await?;
            journal
                .record_storage_network_removed(&claim, &proof)
                .await?;
            journal.validate_jit_conflict_claim(recovery).await
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

pub(crate) async fn remove_owned_network_for_unbound_retirement(
    docker: &Docker,
    journal: &ConnectionJournal,
    engine: &SelectedEngineContext,
    retirement: &UnboundRetirementClaim,
) -> Result<(), HostError> {
    journal.renew_unbound_retirement_claim(retirement).await?;
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
    None,
    Unbound(&'a UnboundRetirementClaim),
    Retirement(&'a PostJitRetirementClaim),
    Conflict(&'a JitConflictRecoveryClaim),
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
    let observed_id = match &authority {
        RemovalAuthority::Unbound(retirement) => {
            journal
                .unbound_creation_target(retirement, UnboundResourceKind::Network, "private")
                .await?
        }
        _ => None,
    };
    let effective_id = claim.network_id().or(observed_id.as_deref());
    let target_id = resolve_target_id(docker, claim, effective_id, deadline).await?;
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

async fn validate_authority(
    journal: &ConnectionJournal,
    authority: &RemovalAuthority<'_>,
) -> Result<(), HostError> {
    match authority {
        RemovalAuthority::None => {}
        RemovalAuthority::Unbound(retirement) => {
            journal
                .validate_unbound_retirement_claim(retirement)
                .await?;
        }
        RemovalAuthority::Retirement(retirement) => {
            journal
                .validate_post_jit_retirement_claim(retirement)
                .await?;
        }
        RemovalAuthority::Conflict(recovery) => {
            journal.validate_jit_conflict_claim(recovery).await?;
        }
    }
    Ok(())
}
