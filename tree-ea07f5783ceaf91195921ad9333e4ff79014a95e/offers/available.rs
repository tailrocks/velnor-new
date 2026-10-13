//! Durable selection, Acquire, JIT, and `PairSink` start for Available offers.

mod effects;

use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::sync::Arc;

use velnor_runner_github::policy::VerifiedJobTrust;
use velnor_runner_host::RunnerImageProfile;
use velnor_runner_journal::journal::{Journal, ReplayRoute};

use crate::linux::LinuxLaunchContext;
use crate::linux::capacity::{
    AvailableLaunchIdentity, LaunchEffectOutcome, ReserveOutcome, reserve_available,
};

use super::super::{
    ActiveSession, ShutdownGate, cutoff, observe_cutoff, protocol_call, protocol_read,
};

pub(super) struct AvailableWork<'a> {
    pub(super) context: &'a LinuxLaunchContext,
    pub(super) journal: &'a Journal,
    pub(super) docker: &'a bollard::Docker,
    pub(super) active: &'a ActiveSession,
    pub(super) shutdown: ShutdownGate<'a>,
}

struct AvailableRuntime<'a> {
    work: AvailableWork<'a>,
    route: ReplayRoute<'a>,
    session_id: String,
    maximum: NonZeroU32,
    profile: RunnerImageProfile,
}

enum OfferOutcome {
    Started,
    CapacityFull,
    NotDispatched,
    Held,
}

/// Process each verified offer once; unsubmitted excess offers are explicitly
/// left unrequested so the message can advance after selected work is durable.
pub(super) async fn process_all(
    mut work: AvailableWork<'_>,
    offers: Vec<VerifiedJobTrust>,
    free_slots: u32,
) -> bool {
    if !unique_request_ids(&offers) {
        return false;
    }
    let selected_count = usize::try_from(free_slots)
        .unwrap_or(usize::MAX)
        .min(offers.len());
    let (selected, deferred) = split_offers(offers, selected_count);
    for trust in deferred {
        if !leave_unrequested(&mut work, trust).await {
            return false;
        }
    }
    let Some(runtime) = available_runtime(work).await else {
        return false;
    };
    Box::pin(process_selected(runtime, selected)).await
}

async fn available_runtime(mut work: AvailableWork<'_>) -> Option<AvailableRuntime<'_>> {
    let route = session_route(work.active)?;
    let read = protocol_read(work.active.protocol.clone(), |protocol| {
        protocol.session.session_id().to_owned()
    });
    let session_id = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        read,
    )
    .await?
    .ok()?;
    let profile = work.context.snapshot.runner_image_profile()?;
    let maximum = NonZeroU32::new(work.context.max_jobs().get())?;
    Some(AvailableRuntime {
        work,
        route,
        session_id,
        maximum,
        profile,
    })
}

async fn process_selected(
    mut runtime: AvailableRuntime<'_>,
    selected: Vec<VerifiedJobTrust>,
) -> bool {
    let mut capacity_full = false;
    for trust in selected {
        observe_cutoff(
            runtime.work.context,
            runtime.work.journal,
            runtime.work.shutdown.receiver,
            runtime.work.shutdown.cutoff,
        )
        .await;
        if runtime.work.shutdown.cutoff.is_some() {
            return false;
        }
        if capacity_full {
            if !leave_unrequested(&mut runtime.work, trust).await {
                return false;
            }
            continue;
        }
        match Box::pin(run_offer(&mut runtime, trust)).await {
            OfferOutcome::Started => {}
            OfferOutcome::CapacityFull => capacity_full = true,
            OfferOutcome::NotDispatched | OfferOutcome::Held => return false,
        }
    }
    true
}

async fn run_offer(runtime: &mut AvailableRuntime<'_>, trust: VerifiedJobTrust) -> OfferOutcome {
    let reservation = cutoff::bounded_persisting(
        runtime.work.journal,
        &mut runtime.work.shutdown,
        runtime.work.context.drain_timeout(),
        None,
        reserve_available(
            runtime.work.journal,
            runtime.route,
            &runtime.session_id,
            &trust,
            runtime.maximum,
        ),
    )
    .await;
    observe_cutoff(
        runtime.work.context,
        runtime.work.journal,
        runtime.work.shutdown.receiver,
        runtime.work.shutdown.cutoff,
    )
    .await;
    let Some(Ok((state, launch))) = reservation else {
        return OfferOutcome::Held;
    };
    match state {
        ReserveOutcome::CapacityFull => {
            return if leave_unrequested(&mut runtime.work, trust).await {
                OfferOutcome::CapacityFull
            } else {
                OfferOutcome::Held
            };
        }
        ReserveOutcome::Reserved => {}
        ReserveOutcome::Draining | ReserveOutcome::Existing => return OfferOutcome::Held,
    }
    let Some(launch) = launch else {
        return OfferOutcome::Held;
    };
    if runtime.work.shutdown.cutoff.is_some() {
        let recorded = cutoff::bounded_persisting(
            runtime.work.journal,
            &mut runtime.work.shutdown,
            runtime.work.context.drain_timeout(),
            None,
            runtime.work.journal.record_launch_no_effect(launch.id),
        )
        .await;
        return match recorded {
            Some(Ok(())) => OfferOutcome::NotDispatched,
            Some(Err(_)) | None => OfferOutcome::Held,
        };
    }
    Box::pin(start_reserved_offer(runtime, launch, trust)).await
}

async fn start_reserved_offer(
    runtime: &mut AvailableRuntime<'_>,
    launch: crate::linux::capacity::ReservedLaunch,
    trust: VerifiedJobTrust,
) -> OfferOutcome {
    let requested_job_id = trust.scale_set_job_id().map(ToOwned::to_owned);
    let identity = AvailableLaunchIdentity {
        message: Some(trust.message_id()),
        request: Some(trust.runner_request_id()),
        workflow_run: Some(trust.workflow_run_id()),
        job: requested_job_id.as_deref(),
    };
    let protocol = Arc::clone(&runtime.work.active.protocol);
    let context = runtime.work.context;
    let journal = runtime.work.journal;
    let shutdown = &mut runtime.work.shutdown;
    let mut operation_receiver = shutdown.receiver.clone();
    let mut operation_cutoff = *shutdown.cutoff;
    let operation_gate = ShutdownGate {
        receiver: &mut operation_receiver,
        cutoff: &mut operation_cutoff,
    };
    let mut wait_receiver = shutdown.receiver.clone();
    let mut wait_cutoff = *shutdown.cutoff;
    let mut wait_gate = ShutdownGate {
        receiver: &mut wait_receiver,
        cutoff: &mut wait_cutoff,
    };
    let mut stage_receiver = shutdown.receiver.clone();
    let mut stage_cutoff = *shutdown.cutoff;
    let docker = runtime.work.docker;
    let jit_for_runner = move |runner_name| {
        let protocol = Arc::clone(&protocol);
        async move {
            effects::acquire_then_jit(
                context,
                journal,
                protocol,
                operation_gate,
                trust,
                runner_name,
            )
            .await
        }
    };
    let launch_future = crate::linux::capacity::run_reserved_worker(
        docker,
        runtime.work.journal,
        &launch,
        &runtime.profile,
        identity,
        jit_for_runner,
        move || async move {
            let gate = ShutdownGate {
                receiver: &mut stage_receiver,
                cutoff: &mut stage_cutoff,
            };
            super::require_before_pair_start(context, journal, gate.receiver, gate.cutoff).await
        },
    );
    let outcome = Box::pin(cutoff::bounded_persisting(
        runtime.work.journal,
        &mut wait_gate,
        runtime.work.context.drain_timeout(),
        None,
        launch_future,
    ))
    .await;
    observe_cutoff(context, journal, shutdown.receiver, shutdown.cutoff).await;
    if matches!(outcome, Some(Ok(LaunchEffectOutcome::Done))) {
        OfferOutcome::Started
    } else {
        OfferOutcome::Held
    }
}

async fn leave_unrequested(work: &mut AvailableWork<'_>, trust: VerifiedJobTrust) -> bool {
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if work.shutdown.cutoff.is_some() {
        return false;
    }
    let dispatch = cutoff::DispatchFence::new();
    let call = protocol_call(
        work.active.protocol.clone(),
        dispatch.clone(),
        work.shutdown.receiver.clone(),
        *work.shutdown.cutoff,
        move |protocol, _transport| {
            protocol
                .admin
                .leave_unrequested_available(&mut protocol.session, &trust)
        },
    );
    matches!(
        cutoff::bounded_protocol(
            work.journal,
            &mut work.shutdown,
            work.context.drain_timeout(),
            None,
            dispatch,
            call,
        )
        .await,
        Some(Ok(()))
    )
}

fn unique_request_ids(offers: &[VerifiedJobTrust]) -> bool {
    let mut seen = BTreeSet::new();
    offers
        .iter()
        .all(|offer| seen.insert(offer.runner_request_id()))
}

fn split_offers(
    offers: Vec<VerifiedJobTrust>,
    selected_count: usize,
) -> (Vec<VerifiedJobTrust>, Vec<VerifiedJobTrust>) {
    let mut selected = Vec::with_capacity(selected_count);
    let mut deferred = Vec::with_capacity(offers.len().saturating_sub(selected_count));
    for (index, offer) in offers.into_iter().enumerate() {
        if index < selected_count {
            selected.push(offer);
        } else {
            deferred.push(offer);
        }
    }
    (selected, deferred)
}

pub(super) fn session_route(active: &ActiveSession) -> Option<ReplayRoute<'_>> {
    let velnor_runner_github::policy::PoolRegistrationScope::Repository { owner, repository } =
        &active.binding.registration_scope
    else {
        return None;
    };
    Some(ReplayRoute {
        destination: "https://api.github.com",
        registration_scope: "repository",
        owner,
        repository,
        runner_group_id: active.binding.actions_runner_group_id,
        runner_group_name: &active.binding.actions_runner_group_name,
        scale_set_id: active.binding.scale_set_id,
        scale_set_name: &active.binding.scale_set_name,
    })
}
