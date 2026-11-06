// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(crate) struct AcquiredRecoveryContext<'a> {
    pub(crate) connection: &'a RepositoryConnection,
    pub(crate) authority_policy: &'a ApprovedJobAuthorityPolicy,
    pub(crate) admission_state: &'a std::path::Path,
    pub(crate) scoped: &'a ConnectionJournal,
    pub(crate) request: &'a AssignmentIdentity,
    pub(crate) reservation_id: i64,
    pub(crate) engine: &'a SelectedEngineContext,
}

pub(crate) async fn recover_acquired_scoped_offer(
    context: AcquiredRecoveryContext<'_>,
    host: &Journal,
    budget: ResourceBudget,
    docker: &Docker,
    release_secret: &ReleaseReadSecret,
) -> Result<ScopedProvisioningOutcome, EnsureError> {
    let engine = context.engine;
    recover_acquired_scoped_offer_with_observation(context, |charge| {
        recovery_observation(docker, host, budget, charge, engine, release_secret)
    })
    .await
}

async fn recover_acquired_scoped_offer_with_observation<F, Fut>(
    context: AcquiredRecoveryContext<'_>,
    observe: F,
) -> Result<ScopedProvisioningOutcome, EnsureError>
where
    F: FnOnce((u64, u64)) -> Fut,
    Fut: Future<Output = Option<crate::launch::RecoveryObservation>>,
{
    let AcquiredRecoveryContext {
        connection,
        authority_policy,
        admission_state,
        scoped,
        request,
        reservation_id,
        engine,
    } = context;
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
