//! One-shot generic JIT effect for a durably reserved assigned-population slot.

use std::sync::Arc;

use velnor_runner_github::{EncodedJit, SessionError};
use velnor_runner_host::HostError;
use velnor_runner_journal::journal::Journal;

use crate::linux::{LinuxLaunchContext, session::ShutdownGate};

use super::super::super::{Protocol, cutoff, observe_cutoff, protocol_call};

pub(super) async fn jit_assigned_demand(
    context: &LinuxLaunchContext,
    journal: &Journal,
    protocol: Arc<std::sync::Mutex<Protocol>>,
    mut gate: ShutdownGate<'_>,
    runner_name: String,
) -> Result<EncodedJit, HostError> {
    observe_cutoff(context, journal, gate.receiver, gate.cutoff).await;
    if gate.cutoff.is_some() {
        return Err(HostError::Journal);
    }
    let dispatch = cutoff::DispatchFence::new();
    let jit = protocol_call(
        protocol,
        dispatch.clone(),
        gate.receiver.clone(),
        *gate.cutoff,
        move |protocol, transport| {
            let result = match protocol.assigned_demand.as_mut() {
                Some(demand) => protocol.admin.jit_assigned_demand(
                    transport,
                    &mut protocol.session,
                    demand,
                    &runner_name,
                ),
                None => Err(SessionError::Uncertain),
            };
            if protocol
                .assigned_demand
                .as_ref()
                .is_some_and(|demand| demand.remaining_jit_count() == 0)
            {
                protocol.assigned_demand = None;
            }
            result
        },
    );
    cutoff::bounded_protocol(
        journal,
        &mut gate,
        context.drain_timeout(),
        None,
        dispatch,
        jit,
    )
    .await
    .and_then(Result::ok)
    .ok_or(HostError::Journal)
}
