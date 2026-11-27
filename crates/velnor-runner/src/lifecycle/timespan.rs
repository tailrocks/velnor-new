// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

const TICKS_PER_SECOND: i128 = 10_000_000;
const FRACTION_DIGITS: usize = 7;
const MAX_NUMBER: u64 = 0x0FFF_FFFF;
const MAX_FRACTION: u64 = 9_999_999;
const MAX_DAYS: u64 = 10_675_199;

struct Fields {
    days: u64,
    hours: u64,
    minutes: u64,
    seconds: u64,
    fraction: i128,
}

#[must_use]
pub fn ticks(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let fields = fields(body)?;
    if fields.hours > 23 || fields.minutes > 59 || fields.seconds > 59 || fields.days > MAX_DAYS {
        return None;
    }
    let seconds = i128::from(fields.days) * 86_400
        + i128::from(fields.hours) * 3_600
        + i128::from(fields.minutes) * 60
        + i128::from(fields.seconds);
    let magnitude = seconds * TICKS_PER_SECOND + fields.fraction;
    i64::try_from(if negative { -magnitude } else { magnitude }).ok()
}

pub(super) fn duration(text: &str) -> Option<Duration> {
    let ticks = u64::try_from(ticks(text)?).unwrap_or(0);
    Some(Duration::from_nanos(ticks.saturating_mul(100)))
}

fn fields(body: &str) -> Option<Fields> {
    let mut numbers = Vec::new();
    let mut shape = String::new();
    let mut digits = body.char_indices().peekable();
    while let Some(&(start, first)) = digits.peek() {
        if !first.is_ascii_digit() {
            return None;
        }
        let mut end = start;
        while let Some(&(at, c)) = digits.peek() {
            if !c.is_ascii_digit() {
                break;
            }
            end = at + 1;
            digits.next();
        }
        numbers.push(&body[start..end]);
        match digits.next() {
            None => break,
            Some((_, c @ ('.' | ':'))) => {
                shape.push(c);
                digits.peek()?;
            }
            Some(_) => return None,
        }
    }
    let (fraction, shape) = match shape.strip_suffix('.') {
        Some(rest @ ("::" | ":::" | ".::")) => (Some(fraction_ticks(numbers.pop()?)?), rest),
        Some(_) => return None,
        None => (None, shape.as_str()),
    };
    let parts = numbers
        .iter()
        .map(|digits| digits.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (days, hours, minutes, seconds) = match (shape, parts.as_slice()) {
        ("", [days]) => (*days, 0, 0, 0),
        (":", [hours, minutes]) => (0, *hours, *minutes, 0),
        ("::", [hours, minutes, seconds]) if fraction.is_some() || *hours <= 23 => {
            (0, *hours, *minutes, *seconds)
        }
        ("::" | ".:", [days, hours, minutes]) => (*days, *hours, *minutes, 0),
        (":::" | ".::", [days, hours, minutes, seconds]) => (*days, *hours, *minutes, *seconds),
        _ => return None,
    };
    Some(Fields {
        days,
        hours,
        minutes,
        seconds,
        fraction: fraction.unwrap_or(0),
    })
}

fn fraction_ticks(digits: &str) -> Option<i128> {
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Some(0);
    }
    let value = significant
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= MAX_NUMBER)?;
    if significant.len() == digits.len() && value > MAX_FRACTION {
        return None;
    }
    let ticks = match digits.len().checked_sub(FRACTION_DIGITS) {
        None | Some(0) => value * 10_u64.pow(u32::try_from(FRACTION_DIGITS - digits.len()).ok()?),
        Some(excess) => u32::try_from(excess)
            .ok()
            .and_then(|excess| 10_u64.checked_pow(excess))
            .map_or(0, |divisor| (value + divisor / 2) / divisor),
    };
    Some(i128::from(ticks))
}
