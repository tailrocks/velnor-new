//! Strict Gregorian date and timestamp parsing for repository freshness records.

/// Parse a strict Gregorian date and return days since the Unix epoch.
pub(crate) fn parse_iso_date(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = parse_digits(text.get(0..4)?, 4)?;
    let month = parse_digits(text.get(5..7)?, 2)?;
    let day = parse_digits(text.get(8..10)?, 2)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day < 1 || day > month_days {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// Parse an ISO date or timezone-aware timestamp into Unix seconds.
pub(crate) fn parse_timestamp(text: &str) -> Option<i64> {
    if let Some(days) = parse_iso_date(text) {
        return days.checked_mul(86_400);
    }
    let split = text.find(['T', 't'])?;
    let days = parse_iso_date(text.get(..split)?)?;
    let (clock, offset) = split_timezone(text.get(split + 1..)?)?;
    let (hour, minute, second) = parse_clock(clock)?;
    let local_seconds = hour
        .checked_mul(3_600)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?;
    days.checked_mul(86_400)?
        .checked_add(local_seconds)?
        .checked_sub(offset)
}

fn parse_utc_offset(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return None;
    }
    let (hours, minutes) = match value.len() {
        6 if bytes[3] == b':' => (
            parse_digits(value.get(1..3)?, 2)?,
            parse_digits(value.get(4..6)?, 2)?,
        ),
        5 => (
            parse_digits(value.get(1..3)?, 2)?,
            parse_digits(value.get(3..5)?, 2)?,
        ),
        _ => return None,
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    let magnitude = hours
        .checked_mul(3_600)?
        .checked_add(minutes.checked_mul(60)?)?;
    if bytes[0] == b'-' {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

fn split_timezone(value: &str) -> Option<(&str, i64)> {
    if let Some(index) = value.find(['+', '-']) {
        let (clock, zone) = value.split_at(index);
        return Some((clock, parse_utc_offset(zone)?));
    }
    for suffix in ['Z', 'z'] {
        if let Some(clock) = value.strip_suffix(suffix) {
            return Some((clock, 0));
        }
    }
    None
}

fn parse_clock(value: &str) -> Option<(i64, i64, i64)> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    let hour = parse_digits(parts[0], 2)?;
    let minute = parse_digits(parts[1], 2)?;
    let second_parts = parts[2].split('.').collect::<Vec<_>>();
    let second = match second_parts.as_slice() {
        [whole] => parse_digits(whole, 2)?,
        [whole, fraction]
            if !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            parse_digits(whole, 2)?
        }
        _ => return None,
    };
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second))
}

fn parse_digits(value: &str, width: usize) -> Option<i64> {
    if value.len() != width || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    year += i64::from(month <= 2);
    (year, month, day)
}

pub(crate) fn iso_date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub(crate) fn iso_timestamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        iso_date(days),
        time / 3_600,
        (time % 3_600) / 60,
        time % 60
    )
}

/// Trim a leading `v` and build metadata for version comparisons.
#[must_use]
pub(crate) fn norm_version(value: &str) -> String {
    value
        .trim()
        .strip_prefix('v')
        .unwrap_or(value.trim())
        .split('+')
        .next()
        .unwrap_or("")
        .to_owned()
}
