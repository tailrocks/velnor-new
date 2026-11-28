// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::rate_signals;

#[test]
fn the_reset_counts_only_when_the_limit_is_exhausted() {
    for status in [403, 429, 503] {
        assert_eq!(
            rate_signals(status, b"\n5\n4102444800").reset,
            None,
            "{status} with requests remaining"
        );
        assert_eq!(
            rate_signals(status, b"\n\n4102444800").reset,
            None,
            "{status} without a remaining count"
        );
        assert_eq!(
            rate_signals(status, b"\n0\n4102444800").reset.as_deref(),
            Some(b"4102444800".as_slice()),
            "{status} with the limit exhausted"
        );
    }
}

#[test]
fn a_403_is_rate_limited_only_by_retry_after_or_an_exhausted_limit() {
    assert!(rate_signals(403, b"7\n\n").rate_limited);
    assert!(rate_signals(403, b"\n0\n").rate_limited);
    assert!(!rate_signals(403, b"\n5\n4102444800").rate_limited);
    assert!(!rate_signals(429, b"\n0\n").rate_limited);
}

#[test]
fn rate_headers_of_other_statuses_are_ignored() {
    let signals = rate_signals(200, b"7\n0\n4102444800");
    assert_eq!((signals.retry_after, signals.reset), (None, None));
}
