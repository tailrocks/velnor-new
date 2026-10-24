//! Continue acquired requests from the current assigned population.

use crate::{
    config::{ApprovedJobAuthorityPolicy, RepositoryConnection},
    error::HostError,
    journal::{AcquiredRecoveryCandidate, AssignmentIdentity, PhysicalRunnerReservation},
};

use super::{PositivePollContext, acquired::start_acquired, pre_jit, successor};
use crate::host_controller::AssignedPopulationAction;
use crate::host_controller::loop_driver::state::PreparedConnection;

#[path = "assigned_population/jit_conflict.rs"]
mod jit_conflict;
#[cfg(test)]
#[path = "assigned_population/missing_statistics.rs"]
mod missing_statistics;
#[path = "assigned_population/retirement.rs"]
mod retirement;

const SCAN_LIMIT: u32 = 64;

struct PopulationPlan {
    actions: Vec<AssignedPopulationAction>,
    recovery_candidates: Vec<AcquiredRecoveryCandidate>,
}

struct RecoveryInputs<'a> {
    connection: &'a RepositoryConnection,
    policy: &'a ApprovedJobAuthorityPolicy,
    candidates: &'a [AcquiredRecoveryCandidate],
}

pub(super) async fn reconcile(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
) -> Result<(), HostError> {
    retirement::reconcile(prepared, context).await;
    if jit_conflict::reconcile(prepared, context).await? {
        return Ok(());
    }
    Box::pin(pre_jit::reconcile(prepared, context)).await?;
    let Some(assigned_total) = prepared.assigned_total else {
        return Ok(());
    };
    let Some(connection) = context.config.connection(&prepared.binding.connection_id) else {
        return Ok(());
    };
    let Some(policy) = context
        .config
        .validated_job_authority_policy(&prepared.binding.connection_id)?
    else {
        return Ok(());
    };
    let population = build_plan(prepared, assigned_total, policy).await?;
    continue_plan(
        prepared,
        context,
        assigned_total,
        connection,
        policy,
        population,
    )
    .await
}

async fn build_plan(
    prepared: &PreparedConnection,
    assigned_total: i64,
    policy: &ApprovedJobAuthorityPolicy,
) -> Result<PopulationPlan, HostError> {
    let reservations = prepared.scope.physical_runner_reservations().await?;
    let acquired = prepared
        .scope
        .acquired_authorized_requests(SCAN_LIMIT)
        .await?;
    let successor_ready = collect_successor_ready(prepared, &acquired, policy).await?;
    let recovery_candidates =
        collect_recovery_candidates(prepared, &reservations, &acquired, policy).await?;
    let plan = crate::host_controller::plan_assigned_population_with_successors(
        assigned_total,
        &prepared.binding.connection_id,
        prepared.set_id,
        &reservations,
        &acquired,
        &recovery_candidates,
        &successor_ready,
    );
    Ok(PopulationPlan {
        actions: plan.actions,
        recovery_candidates,
    })
}

async fn collect_successor_ready(
    prepared: &PreparedConnection,
    acquired: &[AssignmentIdentity],
    policy: &ApprovedJobAuthorityPolicy,
) -> Result<Vec<AssignmentIdentity>, HostError> {
    let mut ready = Vec::new();
    for request in acquired {
        if prepared
            .scope
            .acquired_successor_ready(request, policy.generation(), policy.policy_sha256())
            .await?
        {
            ready.push(request.clone());
        }
    }
    Ok(ready)
}

async fn collect_recovery_candidates(
    prepared: &PreparedConnection,
    reservations: &[PhysicalRunnerReservation],
    acquired: &[AssignmentIdentity],
    policy: &ApprovedJobAuthorityPolicy,
) -> Result<Vec<AcquiredRecoveryCandidate>, HostError> {
    let mut candidates = Vec::new();
    for reservation in reservations {
        let Some(request) = reservation.acquired_request.as_ref() else {
            continue;
        };
        if !is_recoverable_reservation(reservation, request, acquired) {
            continue;
        }
        if let Some(candidate) = prepared
            .scope
            .acquired_recovery_candidate(
                request,
                reservation.reservation_id,
                policy.generation(),
                policy.policy_sha256(),
            )
            .await?
        {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

fn is_recoverable_reservation(
    reservation: &PhysicalRunnerReservation,
    request: &AssignmentIdentity,
    acquired: &[AssignmentIdentity],
) -> bool {
    acquired.contains(request)
        && reservation.reservation.state == crate::journal::ReservationState::Reserved
        && reservation.intent_state == Some(crate::journal::IntentState::Pending)
        && reservation.launch_phase == Some(crate::reconcile::LaunchPhase::ProvisioningHeld)
        && reservation.jit_state == crate::journal::PhysicalRunnerJitState::Absent
}

async fn continue_plan(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
    assigned_total: i64,
    connection: &RepositoryConnection,
    policy: &ApprovedJobAuthorityPolicy,
    population: PopulationPlan,
) -> Result<(), HostError> {
    let PopulationPlan {
        actions,
        recovery_candidates,
    } = population;
    let recovery = RecoveryInputs {
        connection,
        policy,
        candidates: &recovery_candidates,
    };
    for action in actions {
        let (request, reservation_id, candidate_index) = match action {
            AssignedPopulationAction::ContinueExisting {
                request,
                reservation_id,
                candidate_index,
            } => (request, reservation_id, candidate_index),
            AssignedPopulationAction::ReserveSuccessor { request } => {
                successor::reserve_and_start(prepared, context, &request, assigned_total).await?;
                continue;
            }
        };
        recover_candidate(
            prepared,
            context,
            &recovery,
            request,
            reservation_id,
            candidate_index,
        )
        .await?;
    }
    Ok(())
}

async fn recover_candidate(
    prepared: &mut PreparedConnection,
    context: &PositivePollContext<'_>,
    recovery: &RecoveryInputs<'_>,
    request: AssignmentIdentity,
    reservation_id: i64,
    candidate_index: usize,
) -> Result<(), HostError> {
    let candidate = recovery
        .candidates
        .get(candidate_index)
        .ok_or(HostError::Identity)?;
    if candidate.request() != &request || candidate.reservation_id() != reservation_id {
        return Err(HostError::Identity);
    }
    let Some(request_id) = request.request_id() else {
        return Err(HostError::Identity);
    };
    if prepared.pending_acquired_recovery.contains_key(&request_id) {
        return Ok(());
    }
    let outcome = crate::launch::recover_acquired_scoped_offer(
        crate::launch::AcquiredRecoveryContext {
            connection: recovery.connection,
            authority_policy: recovery.policy,
            admission_state: context.state,
            scoped: &prepared.scope,
            request: &request,
            reservation_id,
            engine: context.selected_engine,
        },
        context.journal,
        context.config.resource_budget()?,
        context.docker,
        context.release_read_secret,
    )
    .await;
    match outcome {
        Ok(crate::launch::steps::ScopedProvisioningOutcome::Recovered(recovery)) => {
            start_acquired(prepared, recovery.into_acquired(), context).await?;
        }
        Ok(
            crate::launch::steps::ScopedProvisioningOutcome::NoStart
            | crate::launch::steps::ScopedProvisioningOutcome::KnownNoAcquire(_),
        ) => {}
        Ok(crate::launch::steps::ScopedProvisioningOutcome::Acquired(_)) => {
            return Err(HostError::Ownership);
        }
        Err(error) => {
            eprintln!(
                "The controller cannot continue recovery for this request. connection={} request={} reason={error}",
                prepared.binding.connection_id, request_id
            );
        }
    }
    Ok(())
}
