// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::time::{parse_utc, seconds_between};

#[test]
fn timestamps_across_a_leap_day_and_a_year_end_differ_by_the_calendar() {
    assert_eq!(
        seconds_between(Some("2028-02-28T23:00:00Z"), Some("2028-03-01T01:00:00Z")),
        26 * 3600
    );
    assert_eq!(
        seconds_between(Some("2026-12-31T23:59:59Z"), Some("2027-01-01T00:00:01Z")),
        2
    );
    assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
}

#[test]
fn bad_or_missing_timestamps_give_zero_seconds() {
    assert_eq!(parse_utc("2026-13-01T00:00:00Z"), None);
    assert_eq!(parse_utc("2026-11-12 08:00:00"), None);
    assert_eq!(seconds_between(None, Some("2026-11-12T08:00:00Z")), 0);
    assert_eq!(
        seconds_between(Some("2026-11-12T09:00:00Z"), Some("2026-11-12T08:00:00Z")),
        0
    );
}
