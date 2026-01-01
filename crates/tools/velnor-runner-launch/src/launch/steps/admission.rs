//! Queue-offer classification before acquisition or acknowledgement.

use velnor_runner_github::{Poll, may_ack};
use velnor_runner_host::{EnsureError, Offer, offer};

/// What one poll allows before acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Idle {
    /// HTTP 202. The queue has nothing else.
    Empty,
    /// One `JobAvailable` id. Do not acknowledge yet.
    Launch,
    /// Assigned population is positive. Mint one JIT runner, then acknowledge.
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
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait if may_ack(batch, true) => {
                if absent_census_progress(batch) {
                    Idle::Ack
                } else {
                    match assigned_population(batch) {
                        Some(population) if population > 0 => Idle::Scale,
                        Some(0) => Idle::Ack,
                        Some(_) | None => Idle::Blocked,
                    }
                }
            }
            Offer::Acquire { .. } | Offer::MalformedAvailable | Offer::Wait => Idle::Blocked,
        },
    }
}

fn absent_census_progress(batch: &velnor_runner_github::ParsedBatch) -> bool {
    batch.statistics.is_none()
        && (batch.jobs.is_empty() || velnor_runner_host::assign::progress_only(batch))
}

fn assigned_population(batch: &velnor_runner_github::ParsedBatch) -> Option<i64> {
    batch
        .statistics
        .as_ref()
        .map(velnor_runner_github::Statistics::assigned_population)
        .filter(|population| *population >= 0)
}

pub(in crate::launch) fn assignment(
    polled: &Poll,
) -> Result<Option<(&velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    match offer(polled) {
        Offer::Wait => Ok(None),
        Offer::Acquire { ids, .. } => one_request(batch, &ids),
        Offer::MalformedAvailable => Err(EnsureError::Unexpected {
            status: 0,
            step: "queue message",
        }),
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
