//! Skip one steady message so a later message can be polled.
//!
//! The poll cursor is `lastMessageId`. Cursor zero reads the queue head.
//! A steady head is not acknowledged and is not deleted again. The next poll
//! uses that message id. After [`RETRY`] the head is read once more.

use std::time::{Duration, Instant};

use velnor_runner_github::Poll;

/// Delay before the skipped head is read again.
pub(super) const RETRY: Duration = Duration::from_secs(5);

/// Cursor for this poll. A due retry reads the head.
pub(super) fn poll_cursor(cursor: i64, retry_at: Option<Instant>, now: Instant) -> i64 {
    match retry_at {
        Some(at) if cursor > 0 && now >= at => 0,
        _ => cursor,
    }
}

/// Message id from one poll. Empty is zero.
pub(super) fn message_id(polled: &Poll) -> i64 {
    match polled {
        Poll::Batch(batch) => batch.message_id,
        Poll::Quarantined(batch) => batch.message_id,
        Poll::Empty => 0,
    }
}

/// An empty poll while a head message is skipped must not spin.
pub(super) const fn pause_empty(cursor: i64) -> bool {
    cursor > 0
}

/// `lastMessageId` that skips `message_id`. Zero cannot be a cursor.
pub(super) fn steady_cursor(cursor: i64, message_id: i64) -> Option<i64> {
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
