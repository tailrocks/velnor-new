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
        Some(30)
    );
}

#[test]
fn a_timestamp_without_a_zone_is_a_parse_error_not_a_guess() {
    assert_eq!(parse_utc("2020-01-20T09:42:40"), None);
    assert_eq!(parse_utc("2020-01-20T09:42:40+0800"), None);
    assert_eq!(parse_utc("2020-01-20T09:42:40+25:00"), None);
}

#[test]
fn a_signed_field_or_a_day_the_month_lacks_is_a_parse_error() {
    for text in [
        "2026-11-18T-1:00:00Z",
        "+2026-11-18T00:00:00Z",
        "2026-+1-18T00:00:00Z",
        "2026-11-18T00:+5:00Z",
        "2026-02-31T00:00:00Z",
        "2026-04-31T00:00:00Z",
        "2026-02-29T00:00:00Z",
        "2100-02-29T00:00:00Z",
        "2026-11-18T00:00:00.xZ",
        "2026-11-18T00:00:00.Z",
        "2026-11-18T00:00:00+-1:00",
        "2026-11-18T 0:00:00Z",
    ] {
        assert_eq!(parse_utc(text), None, "{text}");
    }
}

#[test]
fn a_real_leap_day_is_accepted() {
    assert!(parse_utc("2028-02-29T00:00:00Z").is_some());
    assert!(parse_utc("2000-02-29T23:59:59.999Z").is_some());
    assert_eq!(
        parse_utc("2026-03-01T00:00:00Z").map(|march| march - 86_400),
        parse_utc("2026-02-28T00:00:00Z")
    );
}

#[test]
fn a_year_beyond_four_digits_is_a_parse_error_and_never_overflows() {
    for text in [
        "999999999999999999-01-01T00:00:00Z",
        "9223372036854775807-12-31T23:59:59Z",
        "99999-01-01T00:00:00Z",
        "10000-01-01T00:00:00Z",
    ] {
        assert_eq!(parse_utc(text), None, "{text}");
    }
    assert!(parse_utc("9999-12-31T23:59:59Z").is_some());
}
