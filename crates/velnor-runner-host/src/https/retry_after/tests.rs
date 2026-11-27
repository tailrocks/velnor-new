// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{origin_key, record_response, remaining_for};

fn held(base: &str) -> Duration {
    origin_key(base).map_or(Duration::ZERO, |key| remaining_for(&key, Instant::now()))
}

fn within(actual: Duration, seconds: u64) -> bool {
    actual <= Duration::from_secs(seconds)
        && actual + Duration::from_secs(2) > Duration::from_secs(seconds)
}

fn far_reset() -> Result<String, std::time::SystemTimeError> {
    Ok((SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 3_000).to_string())
}

#[test]
fn a_429_without_retry_after_holds_a_minute_and_doubles_while_it_repeats() {
    let base = "https://floor.invalid";
    record_response(base, 429, None, None);
    assert!(within(held(base), 60), "{:?}", held(base));
    record_response(base, 429, None, None);
    assert!(within(held(base), 120), "{:?}", held(base));
}

#[test]
fn an_answer_that_is_not_limited_restarts_the_backoff() {
    let base = "https://recovered.invalid";
    record_response(base, 429, None, None);
    record_response(base, 200, None, None);
    record_response(base, 429, None, None);
    assert!(within(held(base), 60), "{:?}", held(base));
}

#[test]
fn a_503_without_retry_after_holds_nothing() {
    let base = "https://outage.invalid";
    record_response(base, 503, None, None);
    assert_eq!(held(base), Duration::ZERO);
}

#[test]
fn retry_after_wins_over_the_floor() {
    let base = "https://explicit.invalid";
    record_response(base, 429, Some(b"5"), None);
    assert!(within(held(base), 5), "{:?}", held(base));
}

#[test]
fn an_exhausted_limit_holds_until_its_reset() -> Result<(), std::time::SystemTimeError> {
    let base = "https://reset.invalid";
    record_response(base, 503, None, Some(far_reset()?.as_bytes()));
    assert!(within(held(base), 3_000), "{:?}", held(base));
    Ok(())
}
