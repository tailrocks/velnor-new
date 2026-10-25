use super::super::state::PreparedConnection;
use super::PositivePollContext;
use crate::error::HostError;
use crate::journal::{
    AssignmentIdentity, AssignmentKey, PhysicalSuccessorReservationResult, ReservationRequest,
};
use crate::launch::start_permit;

pub(super) async fn reserve_and_start(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
    request: &AssignmentIdentity,
    assigned_total: i64,
) -> Result<(), HostError> {
    if !context.admission_ready || assigned_total <= 0 {
        return Ok(());
    }
    let connection_id = &prepared.binding.connection_id;
    let connection = context
        .config
        .connection(connection_id)
        .ok_or(HostError::Config)?;
    let policy = context
        .config
        .validated_job_authority_policy(connection_id)?
        .ok_or(HostError::Config)?;
    if request.connection_id() != connection_id
        || request.scale_set_id() != prepared.set_id
        || request.request_id().is_none()
    {
        return Err(HostError::Identity);
    }
    let Some(reservation_id) =
        reserve_physical_successor(prepared, context, request, assigned_total, policy).await?
    else {
        return Ok(());
    };
    continue_reserved(
        prepared,
        context,
        connection,
        request,
        reservation_id,
        policy,
    )
    .await
}

async fn reserve_physical_successor(
    prepared: &PreparedConnection,
    context: &PositivePollContext<'_>,
    request: &AssignmentIdentity,
    assigned_total: i64,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
) -> Result<Option<i64>, HostError> {
    #[cfg(test)]
    let permit = match context.start_permit_provider {
        Some(provider) => {
            crate::launch::start_permit_with_test_provider(
                context.docker,
                context.journal,
                context.config.resource_budget()?,
                provider,
            )
            .await
        }
        None => {
            start_permit(
                context.docker,
                context.journal,
                context.config.resource_budget()?,
                context.release_read_secret,
            )
            .await
        }
    };
    #[cfg(not(test))]
    let permit = start_permit(
        context.docker,
        context.journal,
        context.config.resource_budget()?,
        context.release_read_secret,
    )
    .await;
    let Some(permit) = permit else {
        return Ok(None);
    };
    let assigned_limit = u32::try_from(assigned_total).map_err(|_| HostError::Config)?;
    let physical = physical_reservation(request, &permit)?;
    let Some(profile) = prepared.scope.current_storage_profile().await? else {
        return Ok(None);
    };
    let admission = storage_admission(
        &permit,
        &profile,
        &physical.engine_id,
        &physical.docker_root_digest,
    )?;
    let result = prepared
        .scope
        .try_reserve_successor_for_acquired_request(
            ReservationRequest {
                assignment: &physical.identity,
                engine_id: &physical.engine_id,
                docker_root_digest: &physical.docker_root_digest,
                cpu_millicores: physical.cpu_millicores,
                memory_bytes: physical.memory_bytes,
                sample_generation_id: &physical.sample_generation_id,
            },
            permit,
            admission,
            request,
            policy,
            assigned_limit,
        )
        .await?;
    match result {
        PhysicalSuccessorReservationResult::Reserved { reservation_id, .. } => {
            Ok(Some(reservation_id))
        }
        _ => Ok(None),
    }
}

struct SuccessorReservation {
    identity: AssignmentIdentity,
    engine_id: String,
    docker_root_digest: String,
    sample_generation_id: String,
    cpu_millicores: u64,
    memory_bytes: u64,
}

fn physical_reservation(
    request: &AssignmentIdentity,
    permit: &crate::launch::StartPermit,
) -> Result<SuccessorReservation, HostError> {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let identity = request_identity(request, &nonce)?;
    let (cpu_millicores, memory_bytes) = permit.charge();
    let engine_id = permit.engine_id().to_owned();
    let docker_root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    Ok(SuccessorReservation {
        identity,
        engine_id,
        docker_root_digest,
        sample_generation_id,
        cpu_millicores,
        memory_bytes,
    })
}

fn request_identity(
    request: &AssignmentIdentity,
    nonce: &str,
) -> Result<AssignmentIdentity, HostError> {
    AssignmentIdentity::new(
        request.connection_id(),
        request.scale_set_id(),
        AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: nonce.to_owned(),
        },
    )
}

fn storage_admission(
    permit: &crate::launch::StartPermit,
    profile: &crate::journal::StorageProfile,
    engine_id: &str,
    root_digest: &str,
) -> Result<crate::journal::StorageProvisioningAdmission, HostError> {
    permit
        .storage_provisioning_admission(profile, engine_id, root_digest)
        .ok_or(HostError::Identity)
}

async fn continue_reserved(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
    connection: &crate::config::RepositoryConnection,
    request: &AssignmentIdentity,
    reservation_id: i64,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
) -> Result<(), HostError> {
    let Some(candidate) = prepared
        .scope
        .acquired_recovery_candidate(
            request,
            reservation_id,
            policy.generation(),
            policy.policy_sha256(),
        )
        .await?
    else {
        return Err(HostError::Ownership);
    };
    let budget = context.config.resource_budget()?;
    let outcome = recover_candidate(
        prepared, context, connection, request, policy, &candidate, budget,
    )
    .await;
    match outcome {
        Ok(crate::launch::steps::ScopedProvisioningOutcome::Recovered(recovery)) => {
            super::acquired::start_acquired(prepared, recovery.into_acquired(), context).await
        }
        Ok(
            crate::launch::steps::ScopedProvisioningOutcome::NoStart
            | crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(_),
        ) => Ok(()),
        Ok(crate::launch::steps::ScopedProvisioningOutcome::Acquired(_)) => {
            Err(HostError::Ownership)
        }
        Err(error) => {
            eprintln!(
                "The controller cannot continue the acquired request. connection={} request={} reason={}",
                prepared.binding.connection_id,
                request.request_id().unwrap_or_default(),
                error
            );
            Ok(())
        }
    }
}

async fn recover_candidate(
    prepared: &PreparedConnection,
    context: &PositivePollContext<'_>,
    connection: &crate::config::RepositoryConnection,
    request: &AssignmentIdentity,
    policy: &crate::config::ApprovedJobAuthorityPolicy,
    candidate: &crate::journal::AcquiredRecoveryCandidate,
    budget: crate::worker::ResourceBudget,
) -> Result<crate::launch::steps::ScopedProvisioningOutcome, crate::scale_set::EnsureError> {
    #[cfg(test)]
    if let Some(provider) = context.start_permit_provider {
        return crate::launch::recover_acquired_scoped_offer_with_test_provider(
            crate::launch::AcquiredRecoveryContext {
                connection,
                authority_policy: policy,
                admission_state: context.state,
                scoped: &prepared.scope,
                request,
                reservation_id: candidate.reservation_id(),
                engine: context.selected_engine,
            },
            budget,
            context.docker,
            context.journal,
            provider,
        )
        .await;
    }
    crate::launch::recover_acquired_scoped_offer(
        crate::launch::AcquiredRecoveryContext {
            connection,
            authority_policy: policy,
            admission_state: context.state,
            scoped: &prepared.scope,
            request,
            reservation_id: candidate.reservation_id(),
            engine: context.selected_engine,
        },
        context.journal,
        budget,
        context.docker,
        context.release_read_secret,
    )
    .await
}
