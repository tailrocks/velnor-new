//! Durable creation, polling, and exact close of one verified queue session.

use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio::time::{Instant as TokioInstant, timeout_at};
use velnor_runner_github::policy::PollWithTrust;
use velnor_runner_github::policy::PoolBinding;
use velnor_runner_github::{RefreshGate, SessionCloseOutcome};
use velnor_runner_host::DockerDaemonBinding;
use velnor_runner_host::worker::{DiagnosticsStore, list_owned_docker_resources_bound_until};
use velnor_runner_journal::journal::{
    IntentState, Journal, LaunchEffectState, ScaleSetSessionCloseClaim, ScaleSetSessionIdentity,
};
use velnor_runner_launch_slot::holds;

use super::{LinuxLaunchContext, LinuxLaunchCredentials, deadline_after};

mod create;
mod cutoff;
mod deadline_transport;
mod offers;
mod poll_lifecycle;
mod poll_retry;
#[cfg(test)]
mod poll_retry_tests;
mod protocol;
pub(super) use cutoff::{DispatchFence, observe_cutoff_before_deadline};
pub(super) use deadline_transport::{CancelDispatchOnDrop, DeadlineBoundTransport};
use poll_lifecycle::{RecoveryPollWork, process_delivered_batch, process_empty_poll};
use poll_retry::{PollAttempt, SessionPollIo, classify_poll_result, poll_until_delivery};
pub(in crate::linux::session) use protocol::protocol_call;
pub(in crate::linux::session) use protocol::{Protocol, protocol_read};

pub(super) struct ActiveSession {
    binding: PoolBinding,
    docker_binding: DockerDaemonBinding,
    journal_binding: velnor_runner_journal::journal::JournalDockerDaemonBinding,
    identity: ScaleSetSessionIdentity,
    intent_id: i64,
    protocol: std::sync::Arc<std::sync::Mutex<Protocol>>,
}

pub(super) struct ShutdownGate<'a> {
    pub(super) receiver: &'a mut watch::Receiver<Option<Instant>>,
    pub(super) cutoff: &'a mut Option<Instant>,
}

pub(in crate::linux) use create::create_session_if_verified;

pub(super) async fn drive_session(
    context: &LinuxLaunchContext,
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    active: &mut ActiveSession,
    credentials: &LinuxLaunchCredentials,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) {
    if context.snapshot.runner_image_profile().is_none() {
        return;
    }
    let Ok(docker) = velnor_runner_host::connect_unix_bound(&active.docker_binding) else {
        return;
    };
    let mut cursor = 0_i64;
    let mut recovery_budget = offers::RecoveryBudget::default();
    loop {
        // Advance the cursor only after a delivered batch; uncertain retries
        // therefore issue the same side-effect-free queue read.
        let poll = {
            let mut io = SessionPollIo {
                context,
                journal,
                active,
                shutdown,
                cutoff,
            };
            poll_until_delivery(&mut io, cursor).await
        };
        let Some(poll) = poll else { return };
        match poll {
            PollWithTrust::Empty => {
                if !process_empty_poll(RecoveryPollWork {
                    context,
                    journal,
                    diagnostics,
                    docker: &docker,
                    active,
                    credentials,
                    recovery_budget: &mut recovery_budget,
                    shutdown,
                    cutoff,
                })
                .await
                {
                    return;
                }
            }
            PollWithTrust::Batch(batch) => {
                cursor = batch.message_id();
                if !process_delivered_batch(
                    RecoveryPollWork {
                        context,
                        journal,
                        diagnostics,
                        docker: &docker,
                        active,
                        credentials,
                        recovery_budget: &mut recovery_budget,
                        shutdown,
                        cutoff,
                    },
                    batch,
                )
                .await
                {
                    return;
                }
            }
        }
    }
}

async fn poll_once(
    context: &LinuxLaunchContext,
    journal: &Journal,
    active: &ActiveSession,
    cursor: i64,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) -> PollAttempt {
    observe_cutoff(context, journal, shutdown, cutoff).await;
    if cutoff.is_some_and(|deadline| Instant::now() >= deadline) {
        return PollAttempt::Terminal;
    }
    let max_jobs = context.max_jobs.get();
    let dispatch = DispatchFence::new();
    let poll = protocol_call(
        active.protocol.clone(),
        dispatch.clone(),
        shutdown.clone(),
        *cutoff,
        move |protocol, transport| {
            let gate = RefreshGate::new();
            let result = protocol.admin.poll_with_trust(
                transport,
                &mut protocol.session,
                cursor,
                max_jobs,
                &gate,
            );
            Ok(classify_poll_result(result, &gate))
        },
    );
    let mut phase_gate = ShutdownGate {
        receiver: shutdown,
        cutoff,
    };
    let poll = cutoff::bounded_protocol(
        journal,
        &mut phase_gate,
        context.drain_timeout(),
        None,
        dispatch,
        poll,
    )
    .await;
    observe_cutoff(context, journal, phase_gate.receiver, phase_gate.cutoff).await;
    poll.and_then(Result::ok).unwrap_or(PollAttempt::Terminal)
}

async fn close_after_cutoff(
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    active: &mut ActiveSession,
    cutoff: Option<&Instant>,
) -> bool {
    let deadline = cutoff.copied().unwrap_or_else(Instant::now);
    if super::shutdown::cleanup_terminal_workers(
        journal,
        diagnostics,
        &active.docker_binding,
        deadline,
    )
    .await
    .is_err()
    {
        return false;
    }
    close_if_quiescent(journal, active, deadline).await
}

pub(super) async fn close_if_quiescent(
    journal: &Journal,
    active: &mut ActiveSession,
    deadline: Instant,
) -> bool {
    let tokio_deadline = TokioInstant::from_std(deadline);
    let last_message = timeout_at(
        tokio_deadline,
        protocol_read(active.protocol.clone(), |protocol| {
            protocol.session.last_message_id()
        }),
    )
    .await;
    if !matches!(last_message, Ok(Ok(None)))
        || !matches!(
            timeout_at(
                tokio_deadline,
                local_rows_resolved(journal, active.intent_id)
            )
            .await,
            Ok(true)
        )
    {
        return false;
    }
    let Ok(resources) =
        list_owned_docker_resources_bound_until(&active.docker_binding, tokio_deadline).await
    else {
        return false;
    };
    if !resources.is_empty() || Instant::now() >= deadline {
        return false;
    }
    let Ok(Ok(ScaleSetSessionCloseClaim::Claimed(mut permit))) = timeout_at(
        tokio_deadline,
        journal.claim_scale_set_session_close(&active.identity),
    )
    .await
    else {
        return false;
    };
    let close = timeout_at(tokio_deadline, {
        let (_sender, close_shutdown) = watch::channel(Some(deadline));
        protocol_call(
            active.protocol.clone(),
            DispatchFence::new(),
            close_shutdown,
            Some(deadline),
            move |protocol, transport| {
                protocol
                    .admin
                    .close_session_claimed(transport, &mut protocol.session, permit.as_mut())
                    .map(|outcome| (permit, outcome))
            },
        )
    })
    .await;
    let Ok(Ok((permit, SessionCloseOutcome::Closed))) = close else {
        return false;
    };
    matches!(
        timeout_at(
            tokio_deadline,
            journal.record_scale_set_session_closed(&permit)
        )
        .await,
        Ok(Ok(()))
    )
}

pub(super) async fn observe_cutoff(
    context: &LinuxLaunchContext,
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) {
    observe_cutoff_for(journal, shutdown, cutoff, context.drain_timeout).await;
}

pub(super) async fn observe_cutoff_for(
    journal: &Journal,
    shutdown: &mut watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
    drain_timeout: Duration,
) -> bool {
    if cutoff.is_none() {
        match shutdown.has_changed() {
            Ok(true) => *cutoff = *shutdown.borrow_and_update(),
            Err(_) => *cutoff = Some(deadline_after(Instant::now(), drain_timeout)),
            Ok(false) => {}
        }
    }
    if let Some(deadline) = cutoff {
        if Instant::now() >= *deadline {
            return true;
        }
        let _ = super::shutdown::confirm_drain(journal, *deadline).await;
        return true;
    }

    let observed_at = Instant::now();
    let probe_deadline = deadline_after(observed_at, Duration::from_millis(250));
    let drain = tokio::select! {
        result = timeout_at(TokioInstant::from_std(probe_deadline), journal.draining()) => result,
        changed = shutdown.changed() => {
            let latest = *shutdown.borrow_and_update();
            let deadline = match (changed, latest) {
                (Ok(()), Some(value)) => value,
                (Ok(()), None) | (Err(_), _) => deadline_after(Instant::now(), drain_timeout),
            };
            *cutoff = Some(deadline);
            let _ = super::shutdown::confirm_drain(journal, deadline).await;
            return true;
        }
    };
    match drain {
        Ok(Ok(false)) => false,
        Ok(Ok(true)) => {
            *cutoff = Some(deadline_after(observed_at, drain_timeout));
            true
        }
        Ok(Err(_)) | Err(_) => {
            let deadline = deadline_after(observed_at, drain_timeout);
            *cutoff = Some(deadline);
            let _ = super::shutdown::confirm_drain(journal, deadline).await;
            true
        }
    }
}

async fn local_rows_resolved(journal: &Journal, session_id: i64) -> bool {
    let Ok(rows) = journal.rows().await else {
        return false;
    };
    rows.iter().all(|row| {
        if row.id == session_id {
            return row.kind == "scale-set-session"
                && row.state == IntentState::Pending
                && row.launch_effect == LaunchEffectState::MayHaveEffect;
        }
        if row.kind == "launch" {
            return !holds(row);
        }
        if row.kind == "scale-set-session" {
            return row.state == IntentState::Done;
        }
        !matches!(row.state, IntentState::Pending | IntentState::Uncertain)
            || (row.state == IntentState::Failed
                && row.launch_effect == LaunchEffectState::DefiniteNoEffect)
    })
}
