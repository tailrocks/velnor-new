// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const TAG_JOB_BASE_MINUTES: u64 = 30;
pub const MAX_JOB_MINUTES: u64 = 360;

pub(crate) fn seconds(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let amount: u64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let factor = match &rest[..unit] {
            "s" | "sec" | "secs" | "second" | "seconds" => 1,
            "m" | "min" | "mins" | "minute" | "minutes" => 60,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
            "d" | "day" | "days" => 86_400,
            _ => return None,
        };
        total = total.checked_add(amount.checked_mul(factor)?)?;
        rest = rest[unit..].trim_start();
    }
    Some(total)
}

pub(crate) fn minutes(text: &str) -> Option<u64> {
    seconds(text).map(|seconds| seconds.div_ceil(60))
}
