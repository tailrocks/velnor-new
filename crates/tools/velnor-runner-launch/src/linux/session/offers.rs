//! One-message coordination with durable effects before queue acknowledgement.

mod assigned;
mod available;
mod inventory;
mod lifecycle;
mod snapshot;
mod trust;

use std::time::Instant;

use velnor_runner_github::policy::ParsedTrustBatch;
use velnor_runner_github::{Ack, InnerKind, RefreshGate};
use velnor_runner_host::HostError;
use velnor_runner_host::worker::DiagnosticsStore;
use velnor_runner_journal::journal::Journal;

use super::{ActiveSession, ShutdownGate, cutoff, observe_cutoff, protocol_call};
use crate::linux::{LinuxLaunchContext, LinuxLaunchCredentials};

pub(super) struct BatchWork<'a> {
    pub(super) context: &'a LinuxLaunchContext,
    pub(super) journal: &'a Journal,
    pub(super) diagnostics: &'a DiagnosticsStore,
    pub(super) docker: &'a bollard::Docker,
    pub(super) active: &'a ActiveSession,
    pub(super) credentials: &'a LinuxLaunchCredentials,
    pub(super) shutdown: ShutdownGate<'a>,
}

pub(super) async fn require_before_pair_start(
    context: &LinuxLaunchContext,
    journal: &Journal,
    receiver: &mut tokio::sync::watch::Receiver<Option<Instant>>,
    cutoff: &mut Option<Instant>,
) -> Result<(), HostError> {
    observe_cutoff(context, journal, receiver, cutoff).await;
    if cutoff.is_some() {
        Err(HostError::Journal)
    } else {
        Ok(())
    }
}

pub(super) async fn process_batch(mut work: BatchWork<'_>, batch: ParsedTrustBatch) -> bool {
    let current = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        batch_is_current(work.active, &batch),
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if !matches!(current, Some(true)) {
        return false;
    }
    if !snapshot::persist_poll_observation(&mut work, &batch).await {
        return false;
    }
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
    let kinds = BatchKinds::from(&batch);
    let persisted = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        lifecycle::persist_events(work.journal, &batch),
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    let Some(Some(completed)) = persisted else {
        return false;
    };
    if !completed.is_empty()
        && !Box::pin(cleanup_completed(
            work.context,
            work.journal,
            work.diagnostics,
            &work.active.docker_binding,
            &mut work.shutdown,
            &completed,
        ))
        .await
    {
        return false;
    }
    if work.shutdown.cutoff.is_some() || kinds.unsupported || (kinds.available && kinds.assigned) {
        return false;
    }
    let Some(free_slots) = current_free_slots(&mut work).await else {
        return false;
    };
    if !Box::pin(process_kind(&mut work, &batch, &kinds, free_slots)).await {
        return false;
    }
    acknowledge_batch(work, batch.message_id()).await
}

async fn current_free_slots(work: &mut BatchWork<'_>) -> Option<u32> {
    let deadline = Instant::now().checked_add(std::time::Duration::from_secs(20))?;
    cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        Some(deadline),
        inventory::free_slots(work.context, work.journal, work.active, deadline),
    )
    .await?
}

async fn process_kind(
    work: &mut BatchWork<'_>,
    batch: &ParsedTrustBatch,
    kinds: &BatchKinds,
    free_slots: u32,
) -> bool {
    if kinds.available {
        Box::pin(process_available(work, batch, free_slots)).await
    } else if kinds.assigned {
        Box::pin(process_assigned(work, batch, free_slots)).await
    } else {
        true
    }
}

async fn process_available(
    work: &mut BatchWork<'_>,
    batch: &ParsedTrustBatch,
    free_slots: u32,
) -> bool {
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
    let stop_cutoff = *work.shutdown.cutoff;
    let trust_shutdown = work.shutdown.receiver.clone();
    let offers = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        trust::verify_available(
            work.context,
            work.credentials,
            batch,
            stop_cutoff,
            trust_shutdown,
        ),
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    let Some(Some(offers)) = offers else {
        return false;
    };
    if work.shutdown.cutoff.is_some() {
        return false;
    }
    Box::pin(available::process_all(
        available::AvailableWork {
            context: work.context,
            journal: work.journal,
            docker: work.docker,
            active: work.active,
            shutdown: ShutdownGate {
                receiver: &mut *work.shutdown.receiver,
                cutoff: &mut *work.shutdown.cutoff,
            },
        },
        offers,
        free_slots,
    ))
    .await
}

async fn process_assigned(
    work: &mut BatchWork<'_>,
    batch: &ParsedTrustBatch,
    free_slots: u32,
) -> bool {
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if work.shutdown.cutoff.is_some() || batch.statistics().is_none() {
        return false;
    }
    let stop_cutoff = *work.shutdown.cutoff;
    let trust_shutdown = work.shutdown.receiver.clone();
    let verified = cutoff::bounded_persisting(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        trust::verify_assigned(
            work.context,
            work.credentials,
            batch,
            stop_cutoff,
            trust_shutdown,
        ),
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if !matches!(verified, Some(Some(_))) || work.shutdown.cutoff.is_some() {
        return false;
    }
    Box::pin(assigned::process_population(
        work.context,
        work.journal,
        work.docker,
        work.active,
        batch,
        free_slots,
        ShutdownGate {
            receiver: &mut *work.shutdown.receiver,
            cutoff: &mut *work.shutdown.cutoff,
        },
    ))
    .await
}

async fn acknowledge_batch(mut work: BatchWork<'_>, message_id: i64) -> bool {
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    if work
        .shutdown
        .cutoff
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return false;
    }
    let dispatch = cutoff::DispatchFence::new();
    let call = protocol_call(
        work.active.protocol.clone(),
        dispatch.clone(),
        work.shutdown.receiver.clone(),
        *work.shutdown.cutoff,
        move |protocol, transport| {
            protocol.admin.acknowledge_resolved_message(
                transport,
                &mut protocol.session,
                message_id,
                true,
                &RefreshGate::new(),
            )
        },
    );
    let ack = cutoff::bounded_protocol(
        work.journal,
        &mut work.shutdown,
        work.context.drain_timeout(),
        None,
        dispatch,
        call,
    )
    .await;
    observe_cutoff(
        work.context,
        work.journal,
        work.shutdown.receiver,
        work.shutdown.cutoff,
    )
    .await;
    matches!(ack, Some(Ok(Ack::Deleted)))
}

#[derive(Default)]
struct BatchKinds {
    available: bool,
    assigned: bool,
    unsupported: bool,
}

impl BatchKinds {
    fn from(batch: &ParsedTrustBatch) -> Self {
        let mut kinds = Self::default();
        for event in batch.events() {
            match &event.job().kind {
                InnerKind::Available => kinds.available = true,
                InnerKind::Assigned => kinds.assigned = true,
                InnerKind::Completed | InnerKind::Started => {}
                InnerKind::Unsupported(_) => kinds.unsupported = true,
            }
        }
        kinds
    }
}

async fn batch_is_current(active: &ActiveSession, batch: &ParsedTrustBatch) -> bool {
    if batch.message_id() < 0 || batch.source_scale_set_id() != Some(active.binding.scale_set_id) {
        return false;
    }
    let session_id = super::protocol_read(active.protocol.clone(), |protocol| {
        protocol.session.session_id().to_owned()
    })
    .await;
    matches!(session_id, Ok(id) if batch.source_session_id() == Some(id.as_str()))
}

async fn cleanup_completed(
    context: &LinuxLaunchContext,
    journal: &Journal,
    diagnostics: &DiagnosticsStore,
    docker_binding: &velnor_runner_host::DockerDaemonBinding,
    shutdown: &mut ShutdownGate<'_>,
    completed: &[i64],
) -> bool {
    let local_deadline = Instant::now()
        .checked_add(context.drain_timeout())
        .unwrap_or_else(Instant::now);
    let deadline = shutdown
        .cutoff
        .map_or(local_deadline, |cutoff| cutoff.min(local_deadline));
    let cleanup = Box::pin(cutoff::bounded_persisting(
        journal,
        shutdown,
        context.drain_timeout(),
        Some(local_deadline),
        super::super::shutdown::cleanup_terminal_workers(
            journal,
            diagnostics,
            docker_binding,
            deadline,
        ),
    ))
    .await;
    if !matches!(cleanup, Some(Ok(_))) {
        return false;
    }
    let rows = cutoff::bounded_persisting(
        journal,
        shutdown,
        context.drain_timeout(),
        Some(local_deadline),
        journal.rows(),
    )
    .await;
    let Some(Ok(rows)) = rows else {
        return false;
    };
    completed.iter().all(|id| {
        rows.iter()
            .find(|row| row.id == *id)
            .is_some_and(|row| row.cleanup_proven)
    })
}
