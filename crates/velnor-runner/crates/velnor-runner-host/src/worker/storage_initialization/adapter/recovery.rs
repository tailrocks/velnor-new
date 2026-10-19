use std::time::Duration;

use bollard::{
    Docker,
    models::ContainerStateStatusEnum,
    query_parameters::{RemoveContainerOptionsBuilder, StopContainerOptionsBuilder},
};
use tokio::time::{Instant, sleep};

use crate::{
    docker_client::docker_deadline,
    error::HostError,
    journal::{ConnectionJournal, StorageInitializationClaim},
};

use super::super::{
    VerifiedStorageInitializerAbortRemoval, VerifiedStorageInitializerAbsent,
    verification::{
        container_matches_claim, inspect_container, verify_engine, verify_image, verify_volumes,
    },
};
use super::bounded_by_deadline;

/// Complete helper cleanup before the first job container starts.
pub(crate) async fn recover_absent(
    docker: &Docker,
    journal: &ConnectionJournal,
    reservation_id: i64,
    timeout: Duration,
) -> Result<(), HostError> {
    if timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(HostError::DockerTimeout);
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(HostError::DockerTimeout)?;
    let mut active_claim = None;
    let result = bounded_by_deadline(
        deadline,
        Box::pin(async {
            let candidate = journal
                .storage_initializer_abort_candidate(reservation_id)
                .await?
                .ok_or(HostError::Journal)?;
            verify_engine(docker, journal, &candidate).await?;
            verify_volumes(docker, &candidate).await?;
            let claim = journal
                .claim_storage_initializer_abort(&candidate)
                .await?
                .ok_or(HostError::Journal)?;
            active_claim = Some(claim);
            let claim = active_claim.as_ref().ok_or(HostError::Journal)?;
            Box::pin(reconcile_abort(docker, journal, claim)).await
        }),
    )
    .await;
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            mark_abort_uncertain(journal, active_claim.as_ref()).await?;
            Err(error)
        }
        Err(_) => {
            mark_abort_uncertain(journal, active_claim.as_ref()).await?;
            Err(HostError::DockerTimeout)
        }
    }
}

async fn mark_abort_uncertain(
    journal: &ConnectionJournal,
    claim: Option<&StorageInitializationClaim>,
) -> Result<(), HostError> {
    if let Some(claim) = claim {
        journal
            .mark_storage_initializer_abort_uncertain(claim)
            .await?;
    }
    Ok(())
}

async fn reconcile_abort(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
) -> Result<(), HostError> {
    verify_engine(docker, journal, claim).await?;
    verify_volumes(docker, claim).await?;
    let inspected = inspect_container(docker, claim.helper_name()).await?;
    let Some(inspected) = inspected else {
        if let Some(helper_id) = claim.helper_id().or(claim.abort_helper_id())
            && inspect_container(docker, helper_id).await?.is_some()
        {
            return Err(HostError::Ownership);
        }
        return record_absence(journal, claim).await;
    };
    let expected_env = verify_image(docker, claim).await?;
    let helper_id = inspected.id.as_deref().ok_or(HostError::Ownership)?;
    if !valid_container_id(helper_id)
        || claim
            .helper_id()
            .is_some_and(|persisted| persisted != helper_id)
        || claim
            .abort_helper_id()
            .is_some_and(|persisted| persisted != helper_id)
        || !container_matches_claim(&inspected, claim, &expected_env)
    {
        return Err(HostError::Ownership);
    }
    let by_id = inspect_container(docker, helper_id)
        .await?
        .ok_or(HostError::Ownership)?;
    if by_id.id.as_deref() != Some(helper_id)
        || !container_matches_claim(&by_id, claim, &expected_env)
    {
        return Err(HostError::Ownership);
    }
    let proof = VerifiedStorageInitializerAbortRemoval::after_owned_inspection(claim, helper_id);
    journal
        .record_storage_initializer_abort_removal_requested(claim, &proof)
        .await?;
    verify_engine(docker, journal, claim).await?;
    remove_abort_helper(docker, journal, claim, helper_id).await
}

async fn record_absence(
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
) -> Result<(), HostError> {
    let proof = VerifiedStorageInitializerAbsent::after_exact_name_404(claim);
    journal
        .record_storage_initializer_absent(claim, &proof)
        .await
}

async fn remove_abort_helper(
    docker: &Docker,
    journal: &ConnectionJournal,
    claim: &StorageInitializationClaim,
    helper_id: &str,
) -> Result<(), HostError> {
    let options = RemoveContainerOptionsBuilder::default()
        .force(false)
        .v(false)
        .build();
    let expected_env = verify_image(docker, claim).await?;
    for _attempt in 0..32 {
        verify_engine(docker, journal, claim).await?;
        let Some(inspected) = inspect_container(docker, helper_id).await? else {
            return match inspect_container(docker, claim.helper_name()).await? {
                None => record_absence(journal, claim).await,
                Some(_) => Err(HostError::Ownership),
            };
        };
        if inspected.id.as_deref() != Some(helper_id)
            || !container_matches_claim(&inspected, claim, &expected_env)
        {
            return Err(HostError::Ownership);
        }
        let status = inspected
            .state
            .as_ref()
            .and_then(|state| state.status)
            .ok_or(HostError::Ownership)?;
        match status {
            ContainerStateStatusEnum::RUNNING
            | ContainerStateStatusEnum::PAUSED
            | ContainerStateStatusEnum::RESTARTING
            | ContainerStateStatusEnum::STOPPING => {
                verify_engine(docker, journal, claim).await?;
                let stop = StopContainerOptionsBuilder::new().t(1).build();
                let _stop_result =
                    docker_deadline(docker.stop_container(helper_id, Some(stop))).await;
                sleep(Duration::from_millis(25)).await;
            }
            ContainerStateStatusEnum::REMOVING => sleep(Duration::from_millis(25)).await,
            ContainerStateStatusEnum::CREATED
            | ContainerStateStatusEnum::EXITED
            | ContainerStateStatusEnum::DEAD => {
                verify_engine(docker, journal, claim).await?;
                let _remove_result =
                    docker_deadline(docker.remove_container(helper_id, Some(options.clone())))
                        .await;
                match inspect_container(docker, helper_id).await? {
                    None => match inspect_container(docker, claim.helper_name()).await? {
                        None => return record_absence(journal, claim).await,
                        Some(_) => return Err(HostError::Ownership),
                    },
                    Some(after_remove)
                        if after_remove.id.as_deref() == Some(helper_id)
                            && container_matches_claim(&after_remove, claim, &expected_env) =>
                    {
                        sleep(Duration::from_millis(25)).await;
                    }
                    Some(_) => return Err(HostError::Ownership),
                }
            }
            ContainerStateStatusEnum::EMPTY => return Err(HostError::Ownership),
        }
    }
    Err(HostError::Cleanup)
}

fn valid_container_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
