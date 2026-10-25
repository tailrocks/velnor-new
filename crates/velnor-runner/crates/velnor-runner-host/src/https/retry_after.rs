//! Process-local pacing for completed HTTP 429 and 503 responses.
//!
//! A synchronous request can wait only within its existing absolute deadline.
//! The transport API has no cancellation signal, and this state is not durable.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use reqwest::Url;

use super::CurlFail;

const WAIT_SLICE: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct OriginKey(String);

#[derive(Clone, Copy, Debug)]
struct Hold {
    observed_at: Instant,
    duration: Duration,
}

impl Hold {
    fn remaining_at(self, now: Instant) -> Duration {
        self.duration
            .saturating_sub(now.saturating_duration_since(self.observed_at))
    }
}

type Registry = BTreeMap<OriginKey, Hold>;

static HOLDS: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry_lock() -> MutexGuard<'static, Registry> {
    match HOLDS.get_or_init(|| Mutex::new(BTreeMap::new())).lock() {
        Ok(registry) => registry,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn origin_key(base: &str) -> Option<OriginKey> {
    let url = Url::parse(base).ok()?;
    if !matches!(url.scheme(), "https" | "http") {
        return None;
    }
    Some(OriginKey(url.origin().ascii_serialization()))
}

pub(super) fn wait_for_base(base: &str, deadline: Instant) -> Result<(), CurlFail> {
    let Some(key) = origin_key(base) else {
        return if Instant::now() < deadline {
            Ok(())
        } else {
            Err(CurlFail::Timeout)
        };
    };

    loop {
        let now = Instant::now();
        let deadline_left = deadline.saturating_duration_since(now);
        if deadline_left.is_zero() {
            return Err(CurlFail::Timeout);
        }

        let hold_left = remaining_for(&key, now);
        if hold_left.is_zero() {
            return Ok(());
        }

        // Do not hold the registry lock while a caller waits. Short slices let
        // concurrent 429/503 responses extend this origin's hold.
        thread::sleep(hold_left.min(deadline_left).min(WAIT_SLICE));
    }
}

pub(super) fn record_response(base: &str, status: u16, header: Option<&[u8]>) {
    if !matches!(status, 429 | 503) {
        return;
    }
    let (Some(key), Some(header)) = (origin_key(base), header) else {
        return;
    };

    let mut registry = registry_lock();
    let wall_now = SystemTime::now();
    let Some(duration) = parse_retry_after(header, wall_now) else {
        return;
    };
    if duration.is_zero() {
        return;
    }

    // Publish clocks in mutex order so a delayed older sample cannot replace
    // a newer hold with an earlier expiry.
    let now = Instant::now();
    record_at(&mut registry, key, duration, now);
}

fn parse_retry_after(header: &[u8], wall_now: SystemTime) -> Option<Duration> {
    let text = std::str::from_utf8(header)
        .ok()?
        .trim_matches(|character| matches!(character, ' ' | '\t' | '\r' | '\n'));
    if text.is_empty() {
        return None;
    }
    if text.bytes().all(|byte| byte.is_ascii_digit()) {
        return text.parse::<u64>().ok().map(Duration::from_secs);
    }
    if matches!(text.as_bytes().first(), Some(b'+' | b'-')) {
        return None;
    }

    let retry_at = httpdate::parse_http_date(text).ok()?;
    Some(retry_at.duration_since(wall_now).unwrap_or(Duration::ZERO))
}

fn remaining_for(key: &OriginKey, now: Instant) -> Duration {
    let mut registry = registry_lock();
    remaining_at(&mut registry, key, now)
}

fn remaining_at(registry: &mut Registry, key: &OriginKey, now: Instant) -> Duration {
    registry.retain(|_, hold| !hold.remaining_at(now).is_zero());
    registry
        .get(key)
        .copied()
        .map_or(Duration::ZERO, |hold| hold.remaining_at(now))
}

fn record_at(registry: &mut Registry, key: OriginKey, duration: Duration, now: Instant) {
    registry.retain(|_, hold| !hold.remaining_at(now).is_zero());
    if let Some(existing) = registry.get_mut(&key) {
        let incoming = Hold {
            observed_at: now,
            duration,
        };
        let comparison_time = existing.observed_at.max(incoming.observed_at);
        if incoming.remaining_at(comparison_time) > existing.remaining_at(comparison_time) {
            *existing = incoming;
        }
        return;
    }
    registry.insert(
        key,
        Hold {
            observed_at: now,
            duration,
        },
    );
}

#[cfg(test)]
#[path = "retry_after_tests.rs"]
mod tests;
