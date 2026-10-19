use std::time::Instant;

use super::PreparedConnection;
#[cfg(test)]
use super::session_assigned_total;

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

#[cfg(test)]
mod cursor_tests {
    #[cfg(test)]
    use std::time::Duration;
    use std::time::Instant;

    use crate::launch::CursorDisposition;

    use super::{
        apply_cursor_disposition, expire_cursor, record_assigned_total, session_assigned_total,
    };

    #[test]
    fn deferred_head_does_not_block_lifecycle_cursor_progress() {
        let now = Instant::now();
        let (cursor, retry_at) = apply_cursor_disposition(
            0,
            None,
            Some(40),
            CursorDisposition::Deferred { message_id: 40 },
            now,
        );
        assert_eq!(cursor, 40);
        let deadline = retry_at.expect("deferred response has retry deadline");

        // Later lifecycle-only responses are durably recorded then ACKed by
        // positive_poll_once. Keep advancing past them without losing the
        // original unacknowledged Available head or extending its retry.
        let (cursor, retained_deadline) = apply_cursor_disposition(
            cursor,
            retry_at,
            Some(41),
            CursorDisposition::ResetToHead,
            now + Duration::from_secs(1),
        );
        assert_eq!(cursor, 41);
        assert_eq!(retained_deadline, Some(deadline));

        let (cursor, retry_at) = expire_cursor(cursor, retained_deadline, deadline);
        assert_eq!(cursor, 0);
        assert_eq!(retry_at, None);

        // Message ID zero is real, but the pinned client omits its cursor
        // parameter at zero. Do not invent ID one or ACK the offer.
        assert_eq!(
            apply_cursor_disposition(0, None, Some(0), CursorDisposition::Unchanged, now,),
            (0, None)
        );
    }

    #[test]
    fn missing_poll_statistics_retain_the_current_session_snapshot() {
        let mut assigned_total = session_assigned_total(Some(&statistics(4)));
        assert_eq!(assigned_total, Some(4));
        record_assigned_total(&mut assigned_total, Some(6));
        assert_eq!(assigned_total, Some(6));
        record_assigned_total(&mut assigned_total, None);
        assert_eq!(assigned_total, Some(6));

        // A new session uses its own create or refresh response.
        assigned_total = session_assigned_total(Some(&statistics(2)));
        record_assigned_total(&mut assigned_total, None);
        assert_eq!(assigned_total, Some(2));
    }

    fn statistics(total_assigned_jobs: i64) -> velnor_runner_github::Statistics {
        velnor_runner_github::Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 0,
            total_assigned_jobs,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        }
    }
}

pub(super) fn record_assigned_total(current: &mut Option<i64>, sample: Option<i64>) {
    if sample.is_some() {
        *current = sample;
    }
}
