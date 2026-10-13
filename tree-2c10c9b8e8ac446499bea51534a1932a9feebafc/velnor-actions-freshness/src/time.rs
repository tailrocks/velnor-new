//! Strict Gregorian date and timestamp parsing for repository freshness records.

/// Parse a strict Gregorian date and return days since the Unix epoch.
pub(crate) fn parse_iso_date(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = text.get(0..4)?.parse::<i64>().ok()?;
    let month = text.get(5..7)?.parse::<i64>().ok()?;
    let day = text.get(8..10)?.parse::<i64>().ok()?;
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
    let value = text.trim();
    if let Some(days) = parse_iso_date(value) {
        return days.checked_mul(86_400);
    }
    let split = value.find(['T', 't', ' '])?;
    let days = parse_iso_date(&value[..split])?;
    let time = &value[split + 1..];
    let zone_at = time
        .char_indices()
        .skip(1)
        .find_map(|(index, ch)| matches!(ch, '+' | '-').then_some(index));
    let (clock, zone) = if let Some(index) = zone_at {
        (&time[..index], Some(&time[index..]))
    } else if let Some(clock) = time.strip_suffix('Z').or_else(|| time.strip_suffix('z')) {
        (clock, None)
    } else {
        (time, None)
    };
    let parts = clock.split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    let hour = parts[0].parse::<i64>().ok()?;
    let minute = parts[1].parse::<i64>().ok()?;
    let second = parts[2].split('.').next()?.parse::<i64>().ok()?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let offset = zone.map_or(Some(0), parse_utc_offset)?;
    days.checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?
        .checked_sub(offset)
}

fn parse_utc_offset(value: &str) -> Option<i64> {
    let sign = if value.starts_with('-') { -1 } else { 1 };
    let offset = value.get(1..)?;
    let parts = offset.split(':').collect::<Vec<_>>();
    let (hours, minutes) = match parts.as_slice() {
        [hours, minutes] => (hours.parse::<i64>().ok()?, minutes.parse::<i64>().ok()?),
        [digits] if digits.len() == 4 => (
            digits.get(..2)?.parse::<i64>().ok()?,
            digits.get(2..)?.parse::<i64>().ok()?,
        ),
        _ => return None,
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
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
