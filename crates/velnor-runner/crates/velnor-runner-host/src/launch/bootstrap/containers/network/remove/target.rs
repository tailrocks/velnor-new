// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use bollard::Docker;

use crate::{
    error::HostError,
    journal::{ConnectionJournal, StorageNetworkRemovalClaim},
    launch::SelectedEngineContext,
};

use super::super::{inspect_network, request, validate_network, verify_engine};
use super::{RemovalAuthority, validate_authority};

pub(super) async fn resolve_target_id(
    docker: &Docker,
    claim: &StorageNetworkRemovalClaim,
    effective_id: Option<&str>,
    deadline: Instant,
) -> Result<Option<String>, HostError> {
    let by_name = inspect_network(docker, claim.network_name(), deadline).await?;
    if let Some(network) = by_name.as_ref() {
        let actual_id = network
            .id
            .as_deref()
            .filter(|id| super::super::valid_network_id(id))
            .ok_or(HostError::Ownership)?;
        if effective_id.is_some_and(|saved_id| saved_id != actual_id) {
            return Err(HostError::Ownership);
        }
        validate_network(network, claim.network_name(), actual_id, claim.labels())?;
    }

    let target_id = match effective_id {
        Some(saved_id) => {
            if !super::super::valid_network_id(saved_id) {
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

pub(super) async fn remove_and_verify_target(
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
