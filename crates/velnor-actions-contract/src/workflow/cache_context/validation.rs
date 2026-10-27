// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::canonical::normalize_posix_path;
use crate::errors::ContractError;
use crate::ids::is_lower_hex_len;

pub(super) fn require_git_sha(field: &'static str, value: &str) -> Result<(), ContractError> {
    if !is_lower_hex_len(value, 40) {
        return Err(ContractError::identity(field, "malformed_commit"));
    }
    Ok(())
}

pub(super) fn require_absolute_path(field: &'static str, value: &str) -> Result<(), ContractError> {
    let stripped = value
        .strip_prefix('/')
        .ok_or_else(|| ContractError::identity(field, "not_absolute_path"))?;
    if stripped.is_empty()
        || stripped
            .split('/')
            .any(|segment| segment.is_empty() || segment == "..")
        || stripped.bytes().any(|b| b < 0x20 || b == 0x7f)
    {
        return Err(ContractError::identity(field, "invalid_path"));
    }
    Ok(())
}

pub(super) fn require_relative_path(field: &'static str, value: &str) -> Result<(), ContractError> {
    normalize_posix_path(value).map_err(|_| ContractError::identity(field, "invalid_path"))?;
    Ok(())
}

pub(super) fn validate_rfc3339_utc(field: &'static str, value: &str) -> Result<(), ContractError> {
    let invalid = || ContractError::identity(field, "malformed_timestamp");
    let (date, time) = value.split_once('T').ok_or_else(invalid)?;
    let date_parts: Vec<&str> = date.split('-').collect();
    if date_parts.len() != 3
        || date_parts[0].len() != 4
        || date_parts[1].len() != 2
        || date_parts[2].len() != 2
        || !date_parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let time = time.strip_suffix('Z').ok_or_else(invalid)?;
    let (clock, _) = match time.split_once('.') {
        Some((clock, fraction)) => {
            if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            (clock, true)
        }
        None => (time, false),
    };
    let clock_parts: Vec<&str> = clock.split(':').collect();
    if clock_parts.len() != 3
        || clock_parts.iter().any(|part| part.len() != 2)
        || !clock_parts
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let year: u32 = date_parts[0].parse().map_err(|_| invalid())?;
    let month: u32 = date_parts[1].parse().map_err(|_| invalid())?;
    let day: u32 = date_parts[2].parse().map_err(|_| invalid())?;
    let hour: u32 = clock_parts[0].parse().map_err(|_| invalid())?;
    let minute: u32 = clock_parts[1].parse().map_err(|_| invalid())?;
    let second: u32 = clock_parts[2].parse().map_err(|_| invalid())?;
    if year == 0 || !(1..=12).contains(&month) {
        return Err(invalid());
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let last_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap {
                29
            } else {
                28
            }
        }
    };
    if day == 0 || day > last_day {
        return Err(invalid());
    }
    if hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_rfc3339_utc;

    fn accepts(value: &str) -> bool {
        validate_rfc3339_utc("probe", value).is_ok()
    }

    #[test]
    fn rejects_out_of_range_fields() {
        for bad in [
            "2026-99-99T99:99:99Z",
            "2026-00-10T10:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-10-00T00:00:00Z",
            "2026-10-32T00:00:00Z",
            "2026-10-27T24:00:00Z",
            "2026-10-27T10:60:00Z",
            "2026-10-27T10:00:60Z",
            "2026-02-29T00:00:00Z",
            "2023-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "0000-01-01T00:00:00Z",
        ] {
            assert!(!accepts(bad), "accepted out-of-range timestamp {bad}");
        }
    }

    #[test]
    fn accepts_valid_utc_timestamps() {
        for good in [
            "2026-10-27T10:00:00Z",
            "2026-10-26T00:00:00.1Z",
            "2024-02-29T23:59:59Z",
            "2026-01-31T00:00:00Z",
            "2026-12-31T23:59:59.999Z",
        ] {
            assert!(accepts(good), "rejected valid timestamp {good}");
        }
    }
}
