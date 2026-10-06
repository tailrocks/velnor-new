//! What one poll allows. No network and no acknowledgement.

use velnor_runner_github::{InnerKind, ParsedBatch, Poll};

/// One poll outcome the controller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Nothing to acquire. Do not delete an unseen offer.
    Wait,
    /// Acquire these ids before acknowledging `message_id`.
    Acquire {
        /// Poll message id.
        message_id: i64,
        /// `runnerRequestId` values from `JobAvailable` only.
        ids: Vec<i64>,
    },
}

/// `JobAvailable` ids only. Other kinds do not free or acquire a slot.
#[must_use]
pub fn offer(poll: &Poll) -> Offer {
    match poll {
        Poll::Empty => Offer::Wait,
        Poll::Batch(batch) => acquire_offer(batch),
    }
}

fn acquire_offer(batch: &ParsedBatch) -> Offer {
    let ids = available_ids(batch);
    if ids.is_empty() {
        Offer::Wait
    } else {
        Offer::Acquire {
            message_id: batch.message_id,
            ids,
        }
    }
}

/// True when every job is a start or completion notice. An empty batch is not progress.
#[must_use]
pub fn progress_only(batch: &ParsedBatch) -> bool {
    !batch.jobs.is_empty()
        && batch
            .jobs
            .iter()
            .all(|job| matches!(job.kind, InnerKind::Started | InnerKind::Completed))
}

fn available_ids(batch: &ParsedBatch) -> Vec<i64> {
    batch
        .jobs
        .iter()
        .filter(|job| matches!(job.kind, InnerKind::Available))
        .filter_map(|job| job.request_id)
        .collect()
}

#[cfg(test)]
mod tests;
