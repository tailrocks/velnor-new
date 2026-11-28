// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use uuid::Uuid;

const RETRYABLE_STATUSES: [u16; 5] = [429, 500, 502, 503, 504];
const JITTER_MIN_PERCENT: u128 = 80;
const JITTER_SPAN_PERCENT: u128 = 40;

pub(super) const MAX_RETRIES: u32 = 5;

pub(super) fn is_retryable(status: u16) -> bool {
    RETRYABLE_STATUSES.contains(&status)
}

fn requested_delay(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let until = OffsetDateTime::parse(value, &Rfc2822).ok()? - OffsetDateTime::now_utc();
    Duration::try_from(until)
        .ok()
        .filter(|delay| !delay.is_zero())
}

pub(crate) fn random_between(min: Duration, max: Duration) -> Duration {
    let span = max.saturating_sub(min).as_millis();
    if span == 0 {
        return min;
    }
    let jitter = Uuid::new_v4().as_u128() % span;
    min + Duration::from_millis(u64::try_from(jitter).unwrap_or(0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBackoff {
    pub min: Duration,
    pub max: Duration,
    pub coefficient: Duration,
    pub spread: Duration,
}

impl TokenBackoff {
    pub const IMMEDIATE: Self = Self {
        min: Duration::ZERO,
        max: Duration::ZERO,
        coefficient: Duration::ZERO,
        spread: Duration::ZERO,
    };

    pub(super) fn delay(&self, attempt: u32, retry_after: Option<&str>) -> Duration {
        retry_after
            .and_then(|value| self.retry_after(value))
            .unwrap_or_else(|| self.exponential(attempt))
    }

    fn exponential(&self, attempt: u32) -> Duration {
        let jitter = JITTER_MIN_PERCENT + Uuid::new_v4().as_u128() % JITTER_SPAN_PERCENT;
        let steps = (1_u128 << attempt.min(32)) - 1;
        let extra = self.coefficient.as_millis() * steps * jitter / 100;
        let extra = Duration::from_millis(u64::try_from(extra).unwrap_or(u64::MAX));
        self.min.saturating_add(extra).min(self.max)
    }

    fn retry_after(&self, value: &str) -> Option<Duration> {
        let delay = requested_delay(value.trim())?;
        let bound = if delay < self.min {
            self.min
        } else if delay > self.max {
            self.max
        } else {
            return Some(delay);
        };
        Some(random_between(bound, bound.saturating_add(self.spread)))
    }
}

impl Default for TokenBackoff {
    fn default() -> Self {
        Self {
            min: Duration::from_secs(10),
            max: Duration::from_secs(600),
            coefficient: Duration::from_secs(1),
            spread: Duration::from_secs(30),
        }
    }
}
