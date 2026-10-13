//! Classify one queue batch before acquiring or acknowledging it.

use velnor_runner_github::{Poll, may_ack};

use crate::Offer;
use crate::offer;
use crate::scale_set::EnsureError;

/// What one queue poll allows before acquire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Idle {
    /// HTTP 202 with no batch.
    Empty,
    /// One `JobAvailable` request is ready for acquire.
    Launch,
    /// Assigned population is positive and needs a JIT runner.
    Scale,
    /// The batch has no assigned work and can be acknowledged.
    Ack,
    /// The batch is unsupported or lacks safe queue statistics.
    Blocked,
}

/// Classify one poll. Invalid or missing census stays queued.
#[must_use]
pub(crate) fn idle(polled: &Poll) -> Idle {
    match polled {
        Poll::Empty => Idle::Empty,
        Poll::Quarantined(_) => Idle::Blocked,
        Poll::Batch(batch) => match offer(polled) {
            Offer::Acquire { ids, .. } if ids.len() == 1 => Idle::Launch,
            Offer::Wait if may_ack(batch, true) => match assigned_population(batch) {
                Some(population) if population > 0 => Idle::Scale,
                Some(0) => Idle::Ack,
                Some(_) | None => Idle::Blocked,
            },
            Offer::Acquire { .. } | Offer::Wait => Idle::Blocked,
        },
    }
}

/// Extract the one acquired request from a batch.
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

fn assigned_population(batch: &velnor_runner_github::ParsedBatch) -> Option<i64> {
    batch
        .statistics
        .as_ref()
        .map(velnor_runner_github::Statistics::assigned_population)
        .filter(|population| *population >= 0)
}

fn one_request(
    batch: &velnor_runner_github::ParsedBatch,
    ids: &[i64],
) -> Result<Option<(&velnor_runner_github::ParsedBatch, i64)>, EnsureError> {
    if ids.len() == 1 {
        Ok(Some((batch, ids[0])))
    } else {
        Err(EnsureError::Unexpected {
            status: 0,
            step: "capacity",
        })
    }
}
