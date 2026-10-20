//! Continue one acquired request from its existing reservation.

use std::future::Future;

use bollard::Docker;

use crate::{
    config::{ApprovedJobAuthorityPolicy, RepositoryConnection},
    journal::{AssignmentIdentity, ConnectionJournal, Journal},
    launch::{
        ReleaseReadSecret, SelectedEngineContext,
        resource_probe::recovery_observation,
        steps::{CheckedAcquiredRecovery, ScopedProvisioningOutcome},
    },
    scale_set::EnsureError,
    worker::ResourceBudget,
};

/// Continue an acquired request. Do not reserve it again or call `AcquireJobs` again.
pub(crate) async fn recover_acquired_scoped_offer(
    connection: &RepositoryConnection,
    authority_policy: &ApprovedJobAuthorityPolicy,
    admission_state: &std::path::Path,
    scoped: &ConnectionJournal,
    host: &Journal,
    request: &AssignmentIdentity,
    reservation_id: i64,
    budget: ResourceBudget,
    docker: &Docker,
    release_secret: &ReleaseReadSecret,
    engine: &SelectedEngineContext,
) -> Result<ScopedProvisioningOutcome, EnsureError> {
    recover_acquired_scoped_offer_with_observation(
        connection,
        authority_policy,
        admission_state,
        scoped,
        request,
        reservation_id,
        engine,
        |charge| recovery_observation(docker, host, budget, charge, engine, release_secret),
    )
    .await
}

async fn recover_acquired_scoped_offer_with_observation<F, Fut>(
    connection: &RepositoryConnection,
    authority_policy: &ApprovedJobAuthorityPolicy,
    admission_state: &std::path::Path,
    scoped: &ConnectionJournal,
    request: &AssignmentIdentity,
    reservation_id: i64,
    engine: &SelectedEngineContext,
    observe: F,
) -> Result<ScopedProvisioningOutcome, EnsureError>
where
    F: FnOnce((u64, u64)) -> Fut,
    Fut: Future<Output = Option<crate::launch::RecoveryObservation>>,
{
    authority_policy
        .validate_for(connection)
        .map_err(|_| EnsureError::Uncertain)?;
    if authority_policy.connection_id() != &connection.connection_id
        || request.connection_id() != &connection.connection_id
        || request.connection_id() != scoped.connection_id()
    {
        return Err(EnsureError::Uncertain);
    }
    let _admission = crate::launch::lock_connection(admission_state, connection)
        .map_err(|_| EnsureError::Uncertain)?;
    let Some(candidate) = scoped
        .acquired_recovery_candidate(
            request,
            reservation_id,
            authority_policy.generation(),
            authority_policy.policy_sha256(),
        )
        .await
        .map_err(|_| EnsureError::Uncertain)?
    else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    if candidate.engine_id() != engine.engine_id()
        || candidate.docker_root_digest() != engine.docker_root_digest()
    {
        return Ok(ScopedProvisioningOutcome::NoStart);
    }
    let Some(profile) = scoped
        .current_storage_profile()
        .await
        .map_err(|_| EnsureError::Uncertain)?
    else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    if profile.generation_id() != candidate.storage_profile_generation_id()
        || profile.profile_digest() != candidate.storage_profile_digest()
    {
        return Ok(ScopedProvisioningOutcome::NoStart);
    }
    let Some(observation) = observe(candidate.charge()).await else {
        return Ok(ScopedProvisioningOutcome::NoStart);
    };
    if observation
        .storage_admission(&profile, engine.engine_id(), engine.docker_root_digest())
        .is_none()
    {
        return Ok(ScopedProvisioningOutcome::NoStart);
    }
    let checked = CheckedAcquiredRecovery::after_verified_recovery(candidate, observation, engine)
        .map_err(|_| EnsureError::Uncertain)?;
    Ok(ScopedProvisioningOutcome::Recovered(checked))
}

#[cfg(test)]
pub(crate) async fn recover_acquired_scoped_offer_with_test_observation(
    connection: &RepositoryConnection,
    authority_policy: &ApprovedJobAuthorityPolicy,
    admission_state: &std::path::Path,
    scoped: &ConnectionJournal,
    request: &AssignmentIdentity,
    reservation_id: i64,
    engine: &SelectedEngineContext,
    observation: Option<crate::launch::RecoveryObservation>,
) -> Result<ScopedProvisioningOutcome, EnsureError> {
    recover_acquired_scoped_offer_with_observation(
        connection,
        authority_policy,
        admission_state,
        scoped,
        request,
        reservation_id,
        engine,
        |_| async move { observation },
    )
    .await
}
