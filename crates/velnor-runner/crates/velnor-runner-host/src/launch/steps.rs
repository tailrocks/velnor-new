//! Shared poll classification, source authorization, and ACK helpers.

use velnor_runner_github::{Ack, AckScope, Poll, RefreshGate, WireError, ack, may_ack};

use crate::Offer;
use crate::error::HostError;
use crate::listen::map_listen;
use crate::offer;
use crate::scale_set::EnsureError;

mod offer_authorization;
mod scoped;
#[cfg(test)]
pub(crate) use scoped::{RunnerCreateFailure, runner_create_409, runner_create_409_with_jobs};
pub(crate) mod source;
mod source_authorization;

use super::{Drive, Lane};

pub(super) use offer_authorization::authorize_offer;
#[cfg(test)]
pub(super) use offer_authorization::authorize_offer_with_transport;
use source_authorization::source_blocked;

#[cfg(test)]
pub(crate) use scoped::run_native_quarantine_composition;
pub(crate) use scoped::{
    AcquiredProvisioning, CheckedAcquiredRecovery, KnownNoAcquire, ScopedProvisioningOutcome,
    acquire_authorized_request, acquire_scoped,
};

/// What one poll allows before acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Idle {
    /// HTTP 202. The queue has nothing else.
    Empty,
    /// One `JobAvailable` id. Do not acknowledge yet.
    Launch,
    /// Assigned population is positive. Apply current admission rules.
    Scale,
    /// The batch is safe to acknowledge, even when assigned population is unknown.
    /// Deleting it does not free a running slot.
    Ack,
    /// A message that must stay on the queue.
    Blocked,
}

/// Classify one poll. One available id is acquired. A positive current assigned
/// population starts one runner before ack. Missing census allows only empty or
/// start/completion-only batches to be acknowledged; it never implies zero jobs.
/// A batch whose every assigned job already started is acknowledged.
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Quarantined(_) => Idle::Blocked,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait if may_ack(batch, true) => {
                if absent_census_progress(batch)
                    || batch.statistics.is_some() && crate::assign::started_replay(batch)
                {
                    Idle::Ack
                } else {
                    match assigned_population(batch) {
                        Some(population) if population > 0 => Idle::Scale,
                        Some(0) => Idle::Ack,
                        Some(_) | None => Idle::Blocked,
                    }
                }
            }
            Offer::Acquire { .. } | Offer::Wait => Idle::Blocked,
        },
    }
}

/// Subject of a redelivered assigned poll. Other polls have no exception.
#[cfg(test)]
#[must_use]
pub(super) fn mint_subject(polled: &Poll) -> Option<String> {
    let Poll::Batch(batch) = polled else {
        return None;
    };
    if idle(polled) != Idle::Launch {
        return None;
    }
    let (_, request_id) = assignment(polled).ok().flatten()?;
    Some(format!("m{}r{request_id}", batch.message_id))
}

fn absent_census_progress(batch: &velnor_runner_github::ParsedBatch) -> bool {
    batch.statistics.is_none() && (batch.jobs.is_empty() || crate::assign::progress_only(batch))
}

fn assigned_population(batch: &velnor_runner_github::ParsedBatch) -> Option<i64> {
    batch
        .statistics
        .as_ref()
        .map(velnor_runner_github::Statistics::assigned_population)
        .filter(|population| *population >= 0)
}

pub(super) fn assignment(
    polled: &Poll,
) -> Result<Option<(&velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    match offer(polled) {
        Offer::Wait => Ok(None),
        Offer::Acquire { ids, .. } => one_request(batch, &ids),
    }
}

fn one_request<'a>(
    batch: &'a velnor_runner_github::ParsedBatch,
    ids: &[i64],
) -> Result<Option<(&'a velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    if ids.len() == 1 {
        Ok(Some((batch, ids[0])))
    } else {
        Err(EnsureError::Unexpected {
            status: 0,
            step: "capacity",
        })
    }
}
pub(super) fn acknowledge<T>(
    lane: &mut T,
    ctx: &Drive,
    batch: &velnor_runner_github::ParsedBatch,
) -> Result<(), EnsureError>
where
    T: velnor_runner_github::Transport + Lane,
{
    lane.on_queue()?;
    let gate = RefreshGate::new();
    let refresh = || Err(WireError::RefreshExhausted);
    let scope = AckScope {
        replay_safe: true,
        sole_unacquired_offer: false,
        queue_token: &ctx.queue_token,
    };
    let acked = ack(lane, &ctx.queue_path, batch, &scope, &gate, refresh);
    let restored = lane.on_admin();
    let deleted = match acked {
        Ok(Ack::Deleted) => Ok(()),
        Ok(Ack::Suppressed) => Err(EnsureError::Unexpected {
            status: 0,
            step: "ack",
        }),
        Err(error) => Err(map_listen(error)),
    };
    restored?;
    deleted
}

pub(super) fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
