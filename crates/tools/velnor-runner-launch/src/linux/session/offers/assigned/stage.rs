//! Pair staging after one exact assigned-demand ordinal is reserved.

use std::sync::Arc;

use crate::linux::capacity::{
    AvailableLaunchIdentity, LaunchEffectOutcome, run_reserved_worker as run_reserved_launch_worker,
};
use crate::linux::session::{ShutdownGate, cutoff, observe_cutoff};

use super::super::require_before_pair_start;
use super::{AssignedRuntime, DemandSlot, DemandSlotOutcome, effects};

pub(super) async fn run_reserved_worker(
    runtime: &mut AssignedRuntime<'_>,
    slot: &DemandSlot,
    launch: crate::linux::capacity::ReservedLaunch,
) -> DemandSlotOutcome {
    let protocol = Arc::clone(&runtime.active.protocol);
    let context = runtime.context;
    let journal = runtime.journal;
    let shutdown = &mut runtime.shutdown;
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
    let docker = runtime.docker;
    let profile = &runtime.profile;
    let jit_for_runner = move |runner_name| {
        let protocol = Arc::clone(&protocol);
        async move {
            effects::jit_assigned_demand(context, journal, protocol, operation_gate, runner_name)
                .await
        }
    };
    let launch_future = run_reserved_launch_worker(
        docker,
        journal,
        &launch,
        profile,
        AvailableLaunchIdentity {
            message: slot.message_id,
            request: None,
            workflow_run: None,
            job: None,
        },
        jit_for_runner,
        move || async move {
            let gate = ShutdownGate {
                receiver: &mut stage_receiver,
                cutoff: &mut stage_cutoff,
            };
            require_before_pair_start(context, journal, gate.receiver, gate.cutoff).await
        },
    );
    let outcome = Box::pin(cutoff::bounded_persisting(
        runtime.journal,
        &mut wait_gate,
        runtime.context.drain_timeout(),
        None,
        launch_future,
    ))
    .await;
    observe_cutoff(
        runtime.context,
        runtime.journal,
        shutdown.receiver,
        shutdown.cutoff,
    )
    .await;
    if matches!(outcome, Some(Ok(LaunchEffectOutcome::Done))) {
        DemandSlotOutcome::Started
    } else {
        DemandSlotOutcome::Held
    }
}
