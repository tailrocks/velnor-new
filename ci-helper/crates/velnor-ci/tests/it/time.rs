// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::time::{parse_utc, seconds_between};

#[test]
fn numeric_offsets_are_converted_to_utc() {
    assert_eq!(
        parse_utc("2020-01-20T09:42:40.000-08:00"),
        parse_utc("2020-01-20T17:42:40Z")
    );
    assert_eq!(
        parse_utc("2020-01-20T09:42:40+05:30"),
        parse_utc("2020-01-20T04:12:40Z")
    );
    assert_eq!(
        seconds_between(
            Some("2020-01-20T09:00:00-08:00"),
            Some("2020-01-20T17:00:30Z")
        ),
        30
    );
}

#[test]
fn a_timestamp_without_a_zone_is_a_parse_error_not_a_guess() {
    assert_eq!(parse_utc("2020-01-20T09:42:40"), None);
    assert_eq!(parse_utc("2020-01-20T09:42:40+0800"), None);
    assert_eq!(parse_utc("2020-01-20T09:42:40+25:00"), None);
}
