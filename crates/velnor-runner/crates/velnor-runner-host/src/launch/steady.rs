// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use velnor_runner_github::Poll;

pub(super) const RETRY: Duration = Duration::from_secs(5);

pub(super) fn poll_cursor(cursor: i64, retry_at: Option<Instant>, now: Instant) -> i64 {
    match retry_at {
        Some(at) if cursor > 0 && now >= at => 0,
        _ => cursor,
    }
}

pub(super) fn message_id(polled: &Poll) -> i64 {
    match polled {
        Poll::Batch(batch) => batch.message_id,
        Poll::Quarantined(batch) => batch.message_id,
        Poll::Empty => 0,
    }
}

pub(super) const fn pause_empty(cursor: i64) -> bool {
    cursor > 0
}

pub(super) fn steady_cursor(cursor: i64, message_id: i64) -> Option<i64> {
    if message_id > 0 {
        Some(cursor.max(message_id))
    } else {
        None
    }
}
