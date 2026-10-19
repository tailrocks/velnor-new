use std::{future::Future, pin::Pin};

mod claims;
mod handoff;
mod initialize;
mod recovery;
pub(super) use claims::same_start_claim;
use claims::{same_create_claim, verify_claim_image};
pub(crate) use handoff::finish_after_pair_mounts;
pub(crate) use initialize::initialize;
#[cfg(test)]
pub(crate) use recovery::recover_absent;

use bollard::{
    Docker,
    container::AttachContainerResults,
    query_parameters::{
        AttachContainerOptionsBuilder, RemoveContainerOptionsBuilder, StartContainerOptions,
    },
};
use tokio::time::{Instant, timeout_at};

use crate::{
    docker_client::docker_deadline,
    error::HostError,
    journal::{ConnectionJournal, StorageInitializationClaim},
};

use super::{
    StorageInitializationImage, StorageInitializationLimits,
    proof::{
        StorageInitializerCreateResponse, VerifiedStorageInitializerCreated,
        VerifiedStorageInitializerRemoved, VerifiedStorageInitializerStarted,
    },
    specification,
    verification::{
        container_matches_claim, container_matches_claim_identity, inspect_container,
        inspect_owned_container, verify_engine, verify_image, verify_volumes,
    },
};

async fn bounded_by_deadline<T>(
    deadline: Instant,
    future: Pin<Box<dyn Future<Output = T> + Send + '_>>,
) -> Result<T, HostError> {
    timeout_at(deadline, future)
        .await
        .map_err(|_| HostError::DockerTimeout)
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Clone, Copy)]
enum InitializationPhase {
    Create,
    CreateDispatched,
    CreateResponseReceived,
    CreateVerified,
    Start,
    Process,
    Handoff,
}

/// Create and inspect the helper after the journal records the operation claim.
async fn create(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    image: &StorageInitializationImage,
    limits: StorageInitializationLimits,
    phase: &mut InitializationPhase,
    response_evidence: &mut Option<StorageInitializerCreateResponse>,
) -> Result<VerifiedStorageInitializerCreated, HostError> {
    verify_claim_image(claim, image)?;
    verify_engine(docker, journal, claim).await?;
    let expected_env = verify_image(docker, claim).await?;
    verify_volumes(docker, claim).await?;

    let create = specification::specification(
        claim.operation_key(),
        claim.home_volume(),
        claim.work_volume(),
        claim.metadata_volume_name().ok_or(HostError::Identity)?,
        image,
        limits,
    )?;
    if create.options.name.as_deref() != Some(claim.helper_name()) {
        return Err(HostError::Identity);
    }
    verify_engine(docker, journal, claim).await?;
    journal
        .record_storage_initializer_create_dispatched(claim)
        .await?;
    *phase = InitializationPhase::CreateDispatched;
    let response =
        docker_deadline(docker.create_container(Some(create.options), create.config)).await;
    let created = match response {
        Ok(Ok(created)) => {
            *phase = InitializationPhase::CreateResponseReceived;
            created
        }
        Ok(Err(bollard::errors::Error::DockerResponseServerError { .. })) => {
            *phase = InitializationPhase::CreateResponseReceived;
            return Err(HostError::ContainerCreateUncertain);
        }
        _ => return Err(HostError::ContainerCreateUncertain),
    };
    if valid_container_id(&created.id) {
        *response_evidence = Some(StorageInitializerCreateResponse::after_create_response(
            claim,
            &created.id,
        ));
    }
    journal
        .record_storage_initializer_create_response_received(claim)
        .await?;
    if created.id.is_empty() {
        return Err(HostError::ContainerCreateUncertain);
    }
    let response = response_evidence.as_ref().ok_or(HostError::Identity)?;
    if journal
        .record_storage_initializer_create_response(claim, response)
        .await
        .is_err()
    {
        return Err(HostError::ContainerCreateUncertain);
    }
    let Ok(Some(inspected)) = inspect_container(docker, &created.id).await else {
        return Err(HostError::ContainerCreateUncertain);
    };
    if !container_matches_claim(&inspected, claim, &expected_env)
        || inspected.id.as_deref() != Some(created.id.as_str())
    {
        return Err(HostError::Ownership);
    }
    *phase = InitializationPhase::CreateVerified;
    Ok(VerifiedStorageInitializerCreated::from_inspection(
        claim,
        &created.id,
    ))
}

/// Attach before start. Record both operations in order.
async fn start(
    docker: &Docker,
    journal: &ConnectionJournal,
    create_claim: &StorageInitializationClaim,
    created: &VerifiedStorageInitializerCreated,
) -> Result<
    (
        StorageInitializationClaim,
        VerifiedStorageInitializerStarted,
        AttachContainerResults,
    ),
    HostError,
> {
    if !same_create_claim(create_claim, created) {
        return Err(HostError::Identity);
    }
    let start_claim = journal
        .claim_storage_initializer_start(create_claim.reservation_id())
        .await?
        .ok_or(HostError::Journal)?;
    let attached = attach_and_verify_created(docker, journal, &start_claim, created).await?;
    let started = start_container_and_record(docker, journal, &start_claim, created).await?;
    Ok((start_claim, started, attached))
}

async fn attach_and_verify_created(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    created: &VerifiedStorageInitializerCreated,
) -> Result<AttachContainerResults, HostError> {
    let options = AttachContainerOptionsBuilder::new()
        .stdin(true)
        .stdout(true)
        .stderr(true)
        .stream(true)
        .build();
    let Ok(Ok(attached)) =
        docker_deadline(docker.attach_container(created.container_id(), Some(options))).await
    else {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(HostError::ContainerStartUncertain);
    };
    if let Err(error) = verify_engine(docker, journal, claim).await {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(error);
    }
    let inspected = match inspect_owned_container(docker, claim, created.container_id()).await {
        Ok(inspected) => inspected,
        Err(error) => {
            journal
                .mark_storage_initializer_start_uncertain(claim)
                .await?;
            return Err(error);
        }
    };
    if !inspected.state.as_ref().is_some_and(|state| {
        state.status == Some(bollard::models::ContainerStateStatusEnum::CREATED)
            && state.started_at.as_deref() == Some("0001-01-01T00:00:00Z")
    }) {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(HostError::Ownership);
    }
    Ok(attached)
}

async fn start_container_and_record(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    created: &VerifiedStorageInitializerCreated,
) -> Result<VerifiedStorageInitializerStarted, HostError> {
    let Ok(Ok(())) = docker_deadline(
        docker.start_container(created.container_id(), None::<StartContainerOptions>),
    )
    .await
    else {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(HostError::ContainerStartUncertain);
    };
    let Ok(Some(inspected)) = inspect_container(docker, created.container_id()).await else {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(HostError::ContainerStartUncertain);
    };
    if !container_matches_claim_identity(&inspected, claim, created.container_id())
        || !inspected.state.as_ref().is_some_and(|state| {
            matches!(
                state.status,
                Some(
                    bollard::models::ContainerStateStatusEnum::RUNNING
                        | bollard::models::ContainerStateStatusEnum::EXITED
                )
            ) && state
                .started_at
                .as_deref()
                .is_some_and(|s| s != "0001-01-01T00:00:00Z")
        })
    {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(HostError::ContainerStartUncertain);
    }
    let started = VerifiedStorageInitializerStarted::from_inspection(claim, created.container_id());
    if let Err(error) = journal
        .record_storage_initializer_started(claim, &started)
        .await
    {
        journal
            .mark_storage_initializer_start_uncertain(claim)
            .await?;
        return Err(error);
    }
    Ok(started)
}

/// Remove the helper after the journal records the operation claim.
pub(crate) async fn remove(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    started: &VerifiedStorageInitializerStarted,
) -> Result<VerifiedStorageInitializerRemoved, HostError> {
    if !same_start_claim(claim, started) {
        return Err(HostError::Identity);
    }
    let remove_claim = journal
        .claim_storage_initializer_removal(claim.reservation_id())
        .await?
        .ok_or(HostError::Journal)?;
    if let Err(error) = verify_engine(docker, journal, &remove_claim).await {
        journal
            .mark_storage_initializer_removal_uncertain(&remove_claim)
            .await?;
        return Err(error);
    }
    if let Err(error) = inspect_owned_container(docker, &remove_claim, started.container_id()).await
    {
        journal
            .mark_storage_initializer_removal_uncertain(&remove_claim)
            .await?;
        return Err(error);
    }
    let options = RemoveContainerOptionsBuilder::new()
        .force(false)
        .v(false)
        .build();
    let _remove_result =
        docker_deadline(docker.remove_container(started.container_id(), Some(options))).await;
    let Ok(None) = inspect_container(docker, started.container_id()).await else {
        journal
            .mark_storage_initializer_removal_uncertain(&remove_claim)
            .await?;
        return Err(HostError::Cleanup);
    };
    Ok(VerifiedStorageInitializerRemoved::after_absence(
        &remove_claim,
        started.container_id(),
    ))
}

#[cfg(test)]
#[path = "adapter/tests.rs"]
mod tests;
