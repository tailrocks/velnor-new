// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

pub(super) const RENEW_INTERVAL: Duration = Duration::from_mins(20);
pub(super) const RETRY_INTERVAL: Duration = Duration::from_secs(30);

pub(in crate::host_controller::loop_driver) struct AdminRefreshSchedule {
    next_due: Instant,
}

impl AdminRefreshSchedule {
    pub(in crate::host_controller::loop_driver) const fn due_at(now: Instant) -> Self {
        Self { next_due: now }
    }

    pub(in crate::host_controller::loop_driver) fn is_due(&self, now: Instant) -> bool {
        now >= self.next_due
    }

    pub(in crate::host_controller::loop_driver) fn record_success(
        &mut self,
        now: Instant,
        expires_in: Option<Duration>,
    ) {
        let margin = Duration::from_secs(velnor_runner_github::ADMIN_TOKEN_FRESH_MARGIN_SECS);
        let wait = expires_in.map_or(RENEW_INTERVAL, |remaining| {
            remaining.saturating_sub(margin).max(RETRY_INTERVAL)
        });
        self.next_due = now + wait;
    }

    pub(in crate::host_controller::loop_driver) fn record_failure(&mut self, now: Instant) {
        self.next_due = now + RETRY_INTERVAL;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_SECOND: Duration = Duration::from_secs(1);
    const MARGIN: Duration =
        Duration::from_secs(velnor_runner_github::ADMIN_TOKEN_FRESH_MARGIN_SECS);

    #[test]
    fn unknown_expiry_is_due_immediately_then_renews_on_the_interval() {
        let start = Instant::now();
        let mut schedule = AdminRefreshSchedule::due_at(start);
        assert!(schedule.is_due(start));
        schedule.record_success(start, None);
        assert!(!schedule.is_due(start + RENEW_INTERVAL.saturating_sub(ONE_SECOND)));
        assert!(schedule.is_due(start + RENEW_INTERVAL));
    }

    #[test]
    fn known_expiry_is_due_at_the_freshness_margin_before_it() {
        let start = Instant::now();
        let lifetime = Duration::from_secs(3_600);
        let mut schedule = AdminRefreshSchedule::due_at(start);
        schedule.record_success(start, Some(lifetime));
        let due = start + lifetime.saturating_sub(MARGIN);
        assert!(
            !schedule.is_due(start + lifetime.saturating_sub(MARGIN).saturating_sub(ONE_SECOND))
        );
        assert!(schedule.is_due(due));
    }

    #[test]
    fn failure_is_retried_after_the_retry_interval() {
        let start = Instant::now();
        let mut schedule = AdminRefreshSchedule::due_at(start);
        schedule.record_failure(start);
        assert!(!schedule.is_due(start + RETRY_INTERVAL.saturating_sub(ONE_SECOND)));
        assert!(schedule.is_due(start + RETRY_INTERVAL));
    }

    #[test]
    fn repeated_failures_keep_the_retry_interval_fixed() {
        let start = Instant::now();
        let mut schedule = AdminRefreshSchedule::due_at(start);
        let mut now = start;
        for _ in 0..1_000 {
            schedule.record_failure(now);
            assert!(!schedule.is_due(now + RETRY_INTERVAL.saturating_sub(ONE_SECOND)));
            now += RETRY_INTERVAL;
            assert!(schedule.is_due(now));
        }
    }

    #[test]
    fn success_after_failures_returns_to_the_renew_interval() {
        let start = Instant::now();
        let mut schedule = AdminRefreshSchedule::due_at(start);
        schedule.record_failure(start);
        let later = start + RETRY_INTERVAL;
        schedule.record_success(later, None);
        assert!(!schedule.is_due(later + RETRY_INTERVAL));
        assert!(schedule.is_due(later + RENEW_INTERVAL));
    }
}
