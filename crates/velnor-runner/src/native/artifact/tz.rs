// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use jiff::tz::{TimeZone, TimeZoneDatabase};

const DROPPED_FOR_A_DIFFERING_STANDARD_OFFSET: [&str; 4] = ["CET", "EET", "MET", "Cuba"];
const EXEMPT_FROM_THE_DIGIT_RULE: [&str; 4] = ["PST8PDT", "MST7MDT", "CST6CDT", "EST5EDT"];
const LOCALTIME: &str = "/etc/localtime";
const LOCALTIME_ENV: &str = "VELNOR_SYSTEM_LOCALTIME";

fn localtime_path() -> String {
    if cfg!(feature = "test-support") {
        std::env::var(LOCALTIME_ENV).unwrap_or_else(|_| LOCALTIME.to_owned())
    } else {
        LOCALTIME.to_owned()
    }
}

fn system() -> TimeZone {
    std::fs::read(localtime_path())
        .ok()
        .and_then(|bytes| TimeZone::tzif("localtime", &bytes).ok())
        .unwrap_or(TimeZone::UTC)
}

fn listed(database: &TimeZoneDatabase, name: &str) -> bool {
    !DROPPED_FOR_A_DIFFERING_STANDARD_OFFSET.contains(&name)
        && database.available().any(|known| known.as_str() == name)
}

fn ends_the_name(byte: u8) -> bool {
    byte.is_ascii_digit() || byte == b','
}

fn is_whole_hours_offset(text: &str) -> bool {
    let digits = text.bytes().rev().take_while(u8::is_ascii_digit).count();
    (1..=2).contains(&digits) && !text[..text.len() - digits].bytes().any(ends_the_name)
}

fn is_valid_olson_id(raw: &str) -> bool {
    !raw.bytes().any(ends_the_name) || EXEMPT_FROM_THE_DIGIT_RULE.contains(&raw)
}

pub(super) fn zone(step: Option<&str>) -> TimeZone {
    let process = std::env::var_os("TZ").map(|value| value.to_string_lossy().into_owned());
    let Some(raw) = step.or(process.as_deref()) else {
        return system();
    };
    let text = raw.strip_prefix(':').unwrap_or(raw);
    let name = text
        .strip_prefix("posix/")
        .or_else(|| text.strip_prefix("right/"))
        .unwrap_or(text);
    let database = TimeZoneDatabase::bundled();
    if is_valid_olson_id(raw) {
        if listed(&database, name) {
            database.get(name).unwrap_or(TimeZone::UTC)
        } else {
            TimeZone::UTC
        }
    } else if is_whole_hours_offset(text) {
        TimeZone::UTC
    } else {
        system()
    }
}
