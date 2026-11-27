// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;
use zip::DateTime;

const FIRST_YEAR: i32 = 1980;
const LAST_YEAR: u16 = 2043;

fn last() -> DateTime {
    DateTime::from_date_and_time(LAST_YEAR, 12, 31, 23, 59, 58).unwrap_or_default()
}

pub(super) fn dos_time(modified: SystemTime) -> DateTime {
    let Ok(since) = modified.duration_since(UNIX_EPOCH) else {
        return DateTime::default();
    };
    let Ok(at) = i64::try_from(since.as_secs())
        .map_err(|_| ())
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| ()))
    else {
        return last();
    };
    match at.year() {
        ..FIRST_YEAR => DateTime::default(),
        year if year > i32::from(LAST_YEAR) => last(),
        year => u16::try_from(year)
            .ok()
            .and_then(|year| {
                DateTime::from_date_and_time(
                    year,
                    u8::from(at.month()),
                    at.day(),
                    at.hour(),
                    at.minute(),
                    at.second(),
                )
                .ok()
            })
            .unwrap_or_default(),
    }
}
