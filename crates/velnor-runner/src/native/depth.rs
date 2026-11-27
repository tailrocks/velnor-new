// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::num::NonZeroU64;

use crate::broker::Window;

pub(super) fn js_trim(value: &str) -> &str {
    value.trim_matches(|ch: char| ch == '\u{feff}' || (ch.is_whitespace() && ch != '\u{85}'))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Depth {
    Full,
    Last(u64),
}

impl Depth {
    pub(super) fn parse(raw: &str) -> Result<Self, String> {
        let text = js_trim(raw);
        if text.is_empty() {
            return Ok(Self::Last(1));
        }
        let number = js_number(text).floor();
        if number == f64::INFINITY {
            return Err(format!(
                "actions/checkout input 'fetch-depth' must be a finite number, got '{text}'"
            ));
        }
        Ok(if number.is_nan() || number <= 0.0 {
            Self::Full
        } else {
            Self::Last(format!("{number:.0}").parse().unwrap_or(u64::MAX))
        })
    }

    pub(super) fn fetches_through_the_mirror_refs(self) -> bool {
        matches!(self, Self::Last(count) if count > 1)
    }

    pub(super) fn window(self) -> Window {
        match self {
            Self::Last(count) => Window::Last(NonZeroU64::new(count).unwrap_or(NonZeroU64::MIN)),
            Self::Full => Window::All,
        }
    }
}

fn radix_number(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    digits
        .chars()
        .try_fold(0.0, |total, ch| {
            ch.to_digit(radix)
                .map(|digit| total * f64::from(radix) + f64::from(digit))
        })
        .unwrap_or(f64::NAN)
}

fn is_decimal(text: &str) -> bool {
    let (mantissa, exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (text, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let exponent_ok = exponent.is_none_or(|exponent| {
        let digits_only = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        !digits_only.is_empty() && digits(digits_only)
    });
    digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty()) && exponent_ok
}

fn js_number(text: &str) -> f64 {
    for (prefixes, radix) in [(["0x", "0X"], 16), (["0o", "0O"], 8), (["0b", "0B"], 2)] {
        if let Some(digits) = prefixes.iter().find_map(|prefix| text.strip_prefix(prefix)) {
            return radix_number(digits, radix);
        }
    }
    let (sign, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (-1.0, &text[1..]),
        Some(b'+') => (1.0, &text[1..]),
        _ => (1.0, text),
    };
    if unsigned == "Infinity" {
        sign * f64::INFINITY
    } else if is_decimal(unsigned) {
        unsigned
            .parse::<f64>()
            .map_or(f64::NAN, |value| sign * value)
    } else {
        f64::NAN
    }
}

#[cfg(test)]
mod tests;
