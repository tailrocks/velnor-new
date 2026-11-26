// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use crate::launch::steady::{RETRY, poll_cursor, steady_cursor};

#[derive(Debug, Default)]
pub(super) struct Cursor {
    position: i64,
    retry_at: Option<Instant>,
}

impl Cursor {
    pub(super) fn for_poll(&mut self, now: Instant) -> i64 {
        let due = self.retry_at.is_some_and(|at| now >= at);
        self.position = poll_cursor(self.position, self.retry_at, now);
        if due {
            self.retry_at = None;
        }
        self.position
    }

    pub(super) fn deferred(&mut self, message_id: i64, now: Instant) {
        if let Some(position) = steady_cursor(self.position, message_id) {
            self.position = position;
            self.retry_at.get_or_insert(now + RETRY);
        }
    }

    pub(super) fn processed(&mut self, message_id: i64) {
        if self.retry_at.is_none() {
            self.position = 0;
        } else if let Some(position) = steady_cursor(self.position, message_id) {
            self.position = position;
        }
    }
}
