// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use super::PreparedConnection;

impl PreparedConnection {
    pub(in crate::host_controller::loop_driver) fn cursor_for_poll(&mut self, now: Instant) -> i64 {
        (self.poll_cursor, self.poll_retry_at) =
            expire_cursor(self.poll_cursor, self.poll_retry_at, now);
        crate::launch::steady::poll_cursor(self.poll_cursor, self.poll_retry_at, now)
    }

    pub(in crate::host_controller::loop_driver) fn record_cursor_disposition(
        &mut self,
        message_id: Option<i64>,
        disposition: crate::launch::CursorDisposition,
        now: Instant,
    ) {
        (self.poll_cursor, self.poll_retry_at) = apply_cursor_disposition(
            self.poll_cursor,
            self.poll_retry_at,
            message_id,
            disposition,
            now,
        );
    }
}

pub(super) fn record_assigned_total(current: &mut Option<i64>, sample: Option<i64>) {
    if sample.is_some() {
        *current = sample;
    }
}

fn expire_cursor(cursor: i64, retry_at: Option<Instant>, now: Instant) -> (i64, Option<Instant>) {
    if retry_at.is_some_and(|retry_at| now >= retry_at) {
        (0, None)
    } else {
        (cursor, retry_at)
    }
}

fn apply_cursor_disposition(
    cursor: i64,
    retry_at: Option<Instant>,
    message_id: Option<i64>,
    disposition: crate::launch::CursorDisposition,
    now: Instant,
) -> (i64, Option<Instant>) {
    use crate::launch::CursorDisposition;

    match disposition {
        CursorDisposition::Deferred { message_id } => {
            let Some(cursor) = crate::launch::steady::steady_cursor(cursor, message_id) else {
                return (cursor, retry_at);
            };
            (
                cursor,
                Some(retry_at.unwrap_or(now + crate::launch::steady::RETRY)),
            )
        }
        CursorDisposition::ResetToHead if retry_at.is_some() => (
            message_id
                .and_then(|message_id| crate::launch::steady::steady_cursor(cursor, message_id))
                .unwrap_or(cursor),
            retry_at,
        ),
        CursorDisposition::ResetToHead => (0, None),
        CursorDisposition::Unchanged => (cursor, retry_at),
    }
}
