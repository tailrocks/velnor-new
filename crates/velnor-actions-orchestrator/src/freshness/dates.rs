//! Gregorian date and UTC timestamp parsing for freshness evidence.

use std::fmt;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CivilDate {
    pub(super) year: i64,
    pub(super) month: u32,
    pub(super) day: u32,
}

impl fmt::Display for CivilDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

pub(super) fn parse_iso_date(value: &Value) -> Option<CivilDate> {
    let text = value.as_str()?;
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return None;
    }
    let year = text[0..4].parse::<i64>().ok()?;
    let month = text[5..7].parse::<u32>().ok()?;
    let day = text[8..10].parse::<u32>().ok()?;
    valid_date(year, month, day).then_some(CivilDate { year, month, day })
}

pub(super) fn parse_timestamp(value: &Value) -> Option<i64> {
    let text = value.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let normalized = text.replace('Z', "+00:00");
    let separator = normalized.find(['T', 't', ' ']);
    let (date_part, time_part) = separator.map_or((normalized.as_str(), None), |index| {
        (&normalized[..index], Some(&normalized[index + 1..]))
    });
    let date = parse_iso_date(&json!(date_part))?;
    let days = days_from_civil(date.year, date.month, date.day);
    let Some(time_part) = time_part else {
        return Some(days * 86_400_000_000);
    };
    parse_time_micros(time_part).map(|time| days * 86_400_000_000 + time)
}

pub(super) fn current_date(now_micros: i64) -> CivilDate {
    let days = now_micros.div_euclid(86_400_000_000);
    civil_from_days(days)
}

pub(super) fn format_utc(now_micros: i64) -> String {
    let day_micros = now_micros.rem_euclid(86_400_000_000);
    let date = current_date(now_micros);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        date.year,
        date.month,
        date.day,
        day_micros / 3_600_000_000,
        (day_micros / 60_000_000) % 60,
        (day_micros / 1_000_000) % 60
    )
}

fn valid_date(year: i64, month: u32, day: u32) -> bool {
    if year < 1 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days
}

fn parse_time_micros(text: &str) -> Option<i64> {
    let (clock, offset) = parse_offset(text)?;
    let (clock, fractional, has_fraction) = parse_fraction(clock)?;
    let (hour, minute, second, has_seconds) = parse_clock(clock)?;
    if (has_fraction && !has_seconds)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return None;
    }
    Some((((hour * 60 + minute) * 60 + second) * 1_000_000 + fractional) - offset)
}

fn parse_clock(clock: &str) -> Option<(i64, i64, i64, bool)> {
    let mut parts = clock.split(':');
    let hour = parse_two_digits(parts.next()?)?;
    let minute = parse_two_digits(parts.next()?)?;
    let second = parts.next();
    if parts.next().is_some() {
        return None;
    }
    second.map_or(Some((hour, minute, 0, false)), |text| {
        Some((hour, minute, parse_two_digits(text)?, true))
    })
}

fn parse_fraction(text: &str) -> Option<(&str, i64, bool)> {
    let Some(index) = text.find(['.', ',']) else {
        return Some((text, 0, false));
    };
    let clock = &text[..index];
    let fraction = text.get(index + 1..)?;
    if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let microseconds = fraction.get(..fraction.len().min(6))?;
    Some((
        clock,
        format!("{microseconds:0<6}").parse::<i64>().ok()?,
        true,
    ))
}

fn parse_two_digits(text: &str) -> Option<i64> {
    if text.len() != 2 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_offset(text: &str) -> Option<(&str, i64)> {
    if let Some(index) = text.rfind(['+', '-']) {
        let (clock, sign_and_offset) = text.split_at(index);
        let sign = if sign_and_offset.starts_with('-') {
            -1
        } else {
            1
        };
        let raw = &sign_and_offset[1..];
        let (hours, minutes) = if let Some((hours, minutes)) = raw.split_once(':') {
            if minutes.contains(':') {
                return None;
            }
            (hours, minutes)
        } else if raw.len() == 4 {
            (raw.get(..2)?, raw.get(2..)?)
        } else {
            return None;
        };
        let hours = parse_two_digits(hours)?;
        let minutes = parse_two_digits(minutes)?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        Some((clock, sign * (hours * 60 + minutes) * 60 * 1_000_000))
    } else {
        Some((text, 0))
    }
}

pub(super) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> CivilDate {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    CivilDate {
        year,
        month: u32::try_from(month).unwrap_or(1),
        day: u32::try_from(day).unwrap_or(1),
    }
}
