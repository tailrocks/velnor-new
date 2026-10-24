//! Skip one steady message so a later message can be polled.
//!
//! The poll cursor is `lastMessageId`. Cursor zero reads the queue head.
//! A steady head is not acknowledged and is not deleted again. The next poll
//! uses that message id. After [`RETRY`] the head is read once more.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use velnor_runner_github::{InnerKind, Poll};

/// Safe counts from one successfully decoded poll response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PollObservation {
    Complete {
        available_events: u32,
        statistics: Option<PollStatistics>,
    },
    Unavailable,
}

/// The seven scalar counters returned by the Scale Set poll endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PollStatistics {
    pub(crate) available_jobs: i64,
    pub(crate) acquired_jobs: i64,
    pub(crate) assigned_jobs: i64,
    pub(crate) running_jobs: i64,
    pub(crate) registered_runners: i64,
    pub(crate) busy_runners: i64,
    pub(crate) idle_runners: i64,
}

/// Project a poll into event counts and counters without retaining native data.
pub(crate) fn poll_observation(polled: &Poll) -> PollObservation {
    match polled {
        Poll::Empty => PollObservation::Complete {
            available_events: 0,
            statistics: None,
        },
        Poll::Batch(batch) => batch_observation(batch),
        Poll::Quarantined(_) => PollObservation::Unavailable,
    }
}

fn batch_observation(batch: &velnor_runner_github::ParsedBatch) -> PollObservation {
    let available_events = batch
        .jobs
        .iter()
        .filter(|job| job.kind == InnerKind::Available)
        .count();
    let Ok(available_events) = u32::try_from(available_events) else {
        return PollObservation::Unavailable;
    };
    PollObservation::Complete {
        available_events,
        statistics: batch.statistics.as_ref().map(|stats| PollStatistics {
            available_jobs: stats.total_available_jobs,
            acquired_jobs: stats.total_acquired_jobs,
            assigned_jobs: stats.total_assigned_jobs,
            running_jobs: stats.total_running_jobs,
            registered_runners: stats.total_registered_runners,
            busy_runners: stats.total_busy_runners,
            idle_runners: stats.total_idle_runners,
        }),
    }
}

/// Delay before the skipped head is read again.
pub(crate) const RETRY: Duration = Duration::from_secs(5);

/// Assigned job count from this response, if present.
pub(crate) fn assigned_total(polled: &Poll) -> Option<i64> {
    match polled {
        Poll::Batch(batch) => batch
            .statistics
            .as_ref()
            .map(velnor_runner_github::Statistics::assigned_population),
        Poll::Empty | Poll::Quarantined(_) => None,
    }
}

/// Cursor for this poll. A due retry reads the head.
pub(crate) fn poll_cursor(cursor: i64, retry_at: Option<Instant>, now: Instant) -> i64 {
    match retry_at {
        Some(at) if cursor > 0 && now >= at => 0,
        _ => cursor,
    }
}

/// Message id from one poll. Empty is zero.
#[cfg(test)]
pub(super) fn message_id(polled: &Poll) -> i64 {
    match polled {
        Poll::Batch(batch) => batch.message_id,
        Poll::Quarantined(batch) => batch.message_id,
        Poll::Empty => 0,
    }
}

/// Message id from a response. Empty responses have no id.
pub(super) fn response_message_id(polled: &Poll) -> Option<i64> {
    match polled {
        Poll::Batch(batch) => Some(batch.message_id),
        Poll::Quarantined(batch) => Some(batch.message_id),
        Poll::Empty => None,
    }
}

/// An empty poll while a head message is skipped must not spin.
#[cfg(test)]
pub(crate) const fn pause_empty(cursor: i64) -> bool {
    cursor > 0
}

/// `lastMessageId` that skips `message_id`. Zero cannot be a cursor.
pub(crate) fn steady_cursor(cursor: i64, message_id: i64) -> Option<i64> {
    if message_id > 0 {
        Some(cursor.max(message_id))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use velnor_runner_github::{ParsedBatch, Poll};

    use super::{message_id, poll_cursor, steady_cursor};

    #[test]
    fn retry_reads_the_head_again() {
        let now = Instant::now();
        assert_eq!(poll_cursor(42, Some(now), now), 0);
        assert_eq!(poll_cursor(42, Some(now + Duration::from_secs(5)), now), 42);
        assert_eq!(poll_cursor(0, Some(now), now), 0);
    }

    #[test]
    fn steady_skips_only_a_positive_message_id() {
        let polled = Poll::Batch(ParsedBatch {
            message_id: 100_000_789,
            raw_body: String::new(),
            statistics: None,
            jobs: Vec::new(),
        });
        assert_eq!(steady_cursor(0, message_id(&polled)), Some(100_000_789));
        assert_eq!(message_id(&Poll::Empty), 0);
        assert_eq!(steady_cursor(5, 0), None);
        assert_eq!(steady_cursor(100_000_790, 100_000_789), Some(100_000_790));
        assert!(super::pause_empty(100_000_789));
        assert!(!super::pause_empty(0));
    }
}
