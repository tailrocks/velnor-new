// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::preview::json::{Res, require};
use crate::time::days_from_civil;

fn number(text: &[u8]) -> Res<i64> {
    require(!text.is_empty() && text.iter().all(u8::is_ascii_digit))?;
    Ok(text
        .iter()
        .fold(0_i64, |n, digit| n * 10 + i64::from(digit - b'0')))
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `YYYY-MM-DDTHH:MM:SS` as seconds since the epoch: ASCII digits, a real date and a
/// time of day (no leap second).
fn civil(text: &str) -> Res<i64> {
    let b = text.as_bytes();
    require(b.len() == 19 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T')?;
    require(b[13] == b':' && b[16] == b':')?;
    let (year, month, day) = (number(&b[0..4])?, number(&b[5..7])?, number(&b[8..10])?);
    let (hour, minute, second) = (
        number(&b[11..13])?,
        number(&b[14..16])?,
        number(&b[17..19])?,
    );
    require(year >= 1 && (1..=12).contains(&month))?;
    require(day >= 1 && day <= days_in(year, month))?;
    require(hour < 24 && minute < 60 && second < 60)?;
    Ok(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Port of `checked_time`: `YYYY-MM-DDTHH:MM:SSZ`.
pub fn checked_time(text: &str) -> Res<i64> {
    civil(text.strip_suffix('Z').unwrap_or(""))
}

/// The date of an annotated tag, which Python read with `datetime.fromisoformat` and
/// required to be UTC: the same form as [`checked_time`], or with `+00:00` for `Z`.
pub fn tagger_time(text: &str) -> Res<i64> {
    civil(
        text.strip_suffix('Z')
            .or_else(|| text.strip_suffix("+00:00"))
            .unwrap_or(""),
    )
}
