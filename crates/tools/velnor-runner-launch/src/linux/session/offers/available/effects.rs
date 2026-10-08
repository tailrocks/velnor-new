//! One-shot queue effects for a durably reserved Available request.

use std::sync::Arc;

use velnor_runner_github::policy::VerifiedJobTrust;
use velnor_runner_github::{EncodedJit, RefreshGate, SessionError, VerifiedAcquireOutcome};
use velnor_runner_host::HostError;
use velnor_runner_journal::journal::Journal;

use crate::linux::{LinuxLaunchContext, session::ShutdownGate};

use super::super::super::{Protocol, cutoff, observe_cutoff, protocol_call};

pub(super) async fn acquire_then_jit(
    context: &LinuxLaunchContext,
    journal: &Journal,
    protocol: Arc<std::sync::Mutex<Protocol>>,
    mut gate: ShutdownGate<'_>,
    trust: VerifiedJobTrust,
    runner_name: String,
) -> Result<EncodedJit, HostError> {
    observe_cutoff(context, journal, gate.receiver, gate.cutoff).await;
    if gate.cutoff.is_some() {
        return Err(HostError::Journal);
    }
    let dispatch = cutoff::DispatchFence::new();
    let acquire = protocol_call(
        protocol.clone(),
        dispatch.clone(),
        gate.receiver.clone(),
        *gate.cutoff,
        move |protocol, transport| match protocol.admin.acquire_verified(
            transport,
            &mut protocol.session,
            trust,
            &RefreshGate::new(),
        )? {
            VerifiedAcquireOutcome::Acquired(acquired) => Ok(*acquired),
            VerifiedAcquireOutcome::Unresolved { .. } => Err(SessionError::Uncertain),
        },
    );
    let acquired = cutoff::bounded_protocol(
        journal,
        &mut gate,
        context.drain_timeout(),
        None,
        dispatch,
        acquire,
    )
    .await;
    observe_cutoff(context, journal, gate.receiver, gate.cutoff).await;
    let Some(Ok(acquired)) = acquired else {
        return Err(HostError::Journal);
    };
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
            protocol
                .admin
                .jit_verified(transport, &mut protocol.session, acquired, &runner_name)
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
