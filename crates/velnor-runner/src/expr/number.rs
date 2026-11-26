// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/value.rs

pub(crate) fn parse_number(value: &str) -> f64 {
    let value = value.trim();

    if value.is_empty() {
        return 0.0;
    }

    if is_dotnet_double(value)
        && let Ok(parsed) = value.parse::<f64>()
    {
        return parsed;
    }

    if let Some(digits) = value.strip_prefix("0x")
        && !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_hexdigit())
    {
        return u32::from_str_radix(digits, 16).map_or(f64::NAN, signed_bits);
    }

    if let Some(digits) = value.strip_prefix("0o")
        && !digits.is_empty()
        && digits.bytes().all(|c| (b'0'..=b'7').contains(&c))
    {
        return u32::from_str_radix(digits, 8).map_or(f64::NAN, signed_bits);
    }

    match value {
        "Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => f64::NAN,
    }
}

fn signed_bits(bits: u32) -> f64 {
    f64::from(i32::from_ne_bytes(bits.to_ne_bytes()))
}

fn is_dotnet_double(value: &str) -> bool {
    let mut chars = value.chars().peekable();
    if matches!(chars.peek(), Some('+' | '-')) {
        chars.next();
    }

    let mut mantissa_digits = 0usize;
    let mut seen_point = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            mantissa_digits += 1;
            chars.next();
        } else if c == '.' && !seen_point {
            seen_point = true;
            chars.next();
        } else {
            break;
        }
    }

    if mantissa_digits == 0 {
        return false;
    }

    if let Some(&c) = chars.peek() {
        if c != 'e' && c != 'E' {
            return false;
        }
        chars.next();
        if matches!(chars.peek(), Some('+' | '-')) {
            chars.next();
        }
        let mut exponent_digits = 0usize;
        for c in chars.by_ref() {
            if c.is_ascii_digit() {
                exponent_digits += 1;
            } else {
                return false;
            }
        }
        return exponent_digits > 0;
    }

    true
}

pub(crate) fn format_number(value: f64) -> String {
    const SIGNIFICANT_DIGITS: i32 = 15;

    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    let scientific = format!("{value:.14e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or_default();

    if exponent <= -5 || exponent >= SIGNIFICANT_DIGITS {
        let mantissa = trim_trailing_zeros(mantissa);
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}E{sign}{:02}", exponent.abs());
    }

    let decimals = usize::try_from(SIGNIFICANT_DIGITS - 1 - exponent).unwrap_or_default();
    trim_trailing_zeros(&format!("{value:.decimals$}"))
}

fn trim_trailing_zeros(value: &str) -> String {
    if !value.contains('.') {
        return value.to_string();
    }
    let trimmed = value.trim_end_matches('0');
    trimmed.trim_end_matches('.').to_string()
}
