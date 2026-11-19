// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::release_plz::RootConfig;

pub const TAG_JOB_BASE_MINUTES: u16 = 30;
pub(crate) const TOKEN_LIFETIME_MINUTES: u64 = 60;
pub(crate) const TOKEN_MARGIN_MINUTES: u64 = 10;
pub(crate) const DEFAULT_PUBLISH_MINUTES: u64 = 30;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

fn unit_nanos(unit: &str) -> Option<u128> {
    Some(match unit {
        "ns" | "nsec" | "nanos" | "nsecs" | "nanosecond" | "nanoseconds" => 1,
        "us" | "\u{b5}s" | "usec" | "usecs" | "microsecond" | "microseconds" => 1_000,
        "ms" | "msec" | "msecs" | "millis" | "millisecond" | "milliseconds" => 1_000_000,
        "s" | "sec" | "secs" | "second" | "seconds" => NANOS_PER_SECOND,
        "m" | "min" | "mins" | "minute" | "minutes" => 60 * NANOS_PER_SECOND,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600 * NANOS_PER_SECOND,
        "d" | "day" | "days" => 86_400 * NANOS_PER_SECOND,
        "w" | "week" | "weeks" => 604_800 * NANOS_PER_SECOND,
        "M" | "month" | "months" => 2_630_016 * NANOS_PER_SECOND,
        "y" | "year" | "years" => 31_557_600 * NANOS_PER_SECOND,
        _ => return None,
    })
}

fn amount_nanos(whole: &str, fraction: &str, unit: u128) -> Option<u128> {
    let whole: u128 = whole.parse().ok()?;
    let mut total = whole.checked_mul(unit)?;
    let mut scale = unit;
    for digit in fraction.chars() {
        scale /= 10;
        total = total.checked_add(u128::from(digit.to_digit(10)?) * scale)?;
    }
    Some(total)
}

pub(crate) fn nanos(text: &str) -> Option<u128> {
    let mut total = 0u128;
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    if rest == "0" {
        return Some(0);
    }
    while !rest.is_empty() {
        let whole_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let whole = &rest[..whole_end];
        rest = &rest[whole_end..];
        let fraction = match rest.strip_prefix('.') {
            Some(after) => {
                let end = after
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(after.len());
                rest = &after[end..];
                &after[..end]
            }
            None => "",
        };
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        rest = rest.trim_start();
        let unit_end = rest
            .find(|c: char| !c.is_alphabetic())
            .unwrap_or(rest.len());
        let unit = unit_nanos(&rest[..unit_end])?;
        total = total.checked_add(amount_nanos(whole, fraction, unit)?)?;
        rest = rest[unit_end..].trim_start();
    }
    Some(total)
}

pub(crate) fn is_duration(text: &str) -> bool {
    nanos(text).is_some()
}

pub(crate) fn minutes_ceil(nanos: u128) -> u64 {
    u64::try_from(nanos.div_ceil(60 * NANOS_PER_SECOND)).unwrap_or(u64::MAX)
}

pub(crate) fn default_publish_nanos() -> u128 {
    u128::from(DEFAULT_PUBLISH_MINUTES) * 60 * NANOS_PER_SECOND
}

pub(crate) fn publish_budget(config: &RootConfig, inherited: &[String]) -> u128 {
    let publishing = inherited
        .iter()
        .filter(|package| config.publishes(package))
        .count();
    let none_known = config.publishes_to_crates_io && publishing == 0 && config.packages.is_empty();
    let publishing = publishing.max(usize::from(none_known)) as u128;
    config
        .listed_nanos
        .saturating_add(publishing.saturating_mul(config.unlisted_check_nanos))
        .saturating_add(publishing.saturating_mul(config.unlisted_publish_nanos))
}
