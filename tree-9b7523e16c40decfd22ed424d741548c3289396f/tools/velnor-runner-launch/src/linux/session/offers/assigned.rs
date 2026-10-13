//! Generic JIT scaling from one exact assigned/running population snapshot.

mod effects;
mod stage;

use std::num::NonZeroU32;
use std::time::UNIX_EPOCH;

use velnor_runner_github::{SessionError, VerifiedAssignedDemand};
use velnor_runner_host::{HostError, RunnerImageProfile};
use velnor_runner_journal::journal::{Journal, ReplayRoute};

use crate::linux::LinuxLaunchContext;
use crate::linux::capacity::{AssignedSlotIdentity, ReserveOutcome, reserve_assigned_identity};

use super::super::{ActiveSession, ShutdownGate, cutoff, observe_cutoff, protocol_call};
use super::available::session_route;

struct DemandSlot {
    session_id: String,
    demand_id: u64,
    message_id: Option<i64>,
    observed_ms: u128,
    assigned_jobs: u32,
    running_jobs: u32,
    ordinal: u64,
    remaining: u32,
}

struct AssignedRuntime<'a> {
    context: &'a LinuxLaunchContext,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    active: &'a ActiveSession,
    route: ReplayRoute<'a>,
    target_repository_id: i64,
    maximum: NonZeroU32,
    profile: RunnerImageProfile,
    shutdown: ShutdownGate<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DemandSlotOutcome {
    Started,
    NotDispatched,
    Held,
}

/// Use fresh, complete statistics for generic demand. No job/request identity
/// is assigned to any resulting runner; each local slot is reserved first.
pub(super) async fn process_population(
    context: &LinuxLaunchContext,
    journal: &Journal,
    docker: &bollard::Docker,
    active: &ActiveSession,
    batch: &velnor_runner_github::policy::ParsedTrustBatch,
    free_slots: u32,
    shutdown: ShutdownGate<'_>,
) -> bool {
    let Some((assigned, running)) = population(batch) else {
        return false;
    };
    if assigned == running || free_slots == 0 {
        return true;
    }
    observe_cutoff(context, journal, shutdown.receiver, shutdown.cutoff).await;
    if shutdown.cutoff.is_some() {
        return false;
    }
    let Some(mut runtime) = runtime(context, journal, docker, active, shutdown) else {
        return false;
    };
    let demand = take_demand(&mut runtime, free_slots).await;
    let Ok(Some(slot)) = demand else {
        return false;
    };
    Box::pin(run_demand_slots(runtime, slot)).await
}

fn population(batch: &velnor_runner_github::policy::ParsedTrustBatch) -> Option<(u32, u32)> {
    let statistics = batch.statistics()?;
    let assigned = u32::try_from(statistics.total_assigned_jobs).ok()?;
    let running = u32::try_from(statistics.total_running_jobs).ok()?;
    (assigned >= running).then_some((assigned, running))
}

fn runtime<'a>(
    context: &'a LinuxLaunchContext,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    active: &'a ActiveSession,
    shutdown: ShutdownGate<'a>,
) -> Option<AssignedRuntime<'a>> {
    let route = session_route(active)?;
    let target_repository_id = active.binding.repository_id;
    if target_repository_id <= 0 {
        return None;
    }
    let maximum = NonZeroU32::new(context.max_jobs().get())?;
    let profile = context.snapshot.runner_image_profile()?;
    Some(AssignedRuntime {
        context,
        journal,
        docker,
        active,
        route,
        target_repository_id,
        maximum,
        profile,
        shutdown,
    })
}

async fn take_demand(
    runtime: &mut AssignedRuntime<'_>,
    free_slots: u32,
) -> Result<Option<DemandSlot>, HostError> {
    observe_cutoff(
        runtime.context,
        runtime.journal,
        runtime.shutdown.receiver,
        runtime.shutdown.cutoff,
    )
    .await;
    if runtime.shutdown.cutoff.is_some() {
        return Err(HostError::Journal);
    }
    let dispatch = cutoff::DispatchFence::new();
    let call = protocol_call(
        runtime.active.protocol.clone(),
        dispatch.clone(),
        runtime.shutdown.receiver.clone(),
        *runtime.shutdown.cutoff,
        move |protocol, _transport| {
            if protocol.assigned_demand.is_some() {
                return Err(SessionError::Uncertain);
            }
            protocol.assigned_demand = protocol
                .admin
                .take_assigned_demand(&mut protocol.session, free_slots)?;
            protocol
                .assigned_demand
                .as_ref()
                .map(demand_slot)
                .transpose()
        },
    );
    cutoff::bounded_protocol(
        runtime.journal,
        &mut runtime.shutdown,
        runtime.context.drain_timeout(),
        None,
        dispatch,
        call,
    )
    .await
    .ok_or(HostError::Journal)?
}

async fn run_demand_slots(mut runtime: AssignedRuntime<'_>, mut slot: DemandSlot) -> bool {
    while slot.remaining > 0 {
        observe_cutoff(
            runtime.context,
            runtime.journal,
            runtime.shutdown.receiver,
            runtime.shutdown.cutoff,
        )
        .await;
        if runtime.shutdown.cutoff.is_some() {
            return false;
        }
        if Box::pin(run_demand_slot(&mut runtime, &slot)).await != DemandSlotOutcome::Started {
            return false;
        }
        let Some(ordinal) = slot.ordinal.checked_add(1) else {
            return false;
        };
        slot.ordinal = ordinal;
        slot.remaining -= 1;
    }
    true
}

async fn run_demand_slot(
    runtime: &mut AssignedRuntime<'_>,
    slot: &DemandSlot,
) -> DemandSlotOutcome {
    let identity = AssignedSlotIdentity {
        route: runtime.route,
        target_repository_id: runtime.target_repository_id,
        session_id: &slot.session_id,
        demand_id: slot.demand_id,
        message_id: slot.message_id,
        observed_ms: slot.observed_ms,
        assigned_jobs: slot.assigned_jobs,
        running_jobs: slot.running_jobs,
        ordinal: slot.ordinal,
    };
    let reservation = cutoff::bounded_persisting(
        runtime.journal,
        &mut runtime.shutdown,
        runtime.context.drain_timeout(),
        None,
        reserve_assigned_identity(
            runtime.journal,
            identity,
            &runtime.active.journal_binding,
            runtime.maximum,
        ),
    )
    .await;
    observe_cutoff(
        runtime.context,
        runtime.journal,
        runtime.shutdown.receiver,
        runtime.shutdown.cutoff,
    )
    .await;
    let Some(Ok((ReserveOutcome::Reserved, Some(launch)))) = reservation else {
        return DemandSlotOutcome::Held;
    };
    if runtime.shutdown.cutoff.is_some() {
        return match runtime.journal.record_launch_no_effect(launch.id).await {
            Ok(()) => DemandSlotOutcome::NotDispatched,
            Err(_) => DemandSlotOutcome::Held,
        };
    }
    Box::pin(stage::run_reserved_worker(runtime, slot, launch)).await
}

fn demand_slot(demand: &VerifiedAssignedDemand) -> Result<DemandSlot, SessionError> {
    let observed_ms = demand
        .observed_at()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SessionError::Uncertain)?
        .as_millis();
    Ok(DemandSlot {
        session_id: demand.session_id().to_owned(),
        demand_id: demand.demand_id(),
        message_id: demand.message_id(),
        observed_ms,
        assigned_jobs: demand.assigned_jobs(),
        running_jobs: demand.running_jobs(),
        ordinal: u64::from(demand.next_slot_ordinal()),
        remaining: demand.remaining_jit_count(),
    })
}
