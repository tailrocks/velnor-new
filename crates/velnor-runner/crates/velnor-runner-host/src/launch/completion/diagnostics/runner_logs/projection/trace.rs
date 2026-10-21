use serde::Serialize;

use crate::error::HostError;

use super::paths::relative_archive_path;

const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Component {
    Runner,
    Worker,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Category {
    Critical,
    Error,
    Warning,
    Information,
    Verbose,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct Event {
    timestamp: String,
    component: Component,
    category: Category,
}

pub(super) fn parse_log_name(path: &[u8]) -> Result<(Component, String), HostError> {
    let path = relative_archive_path(path);
    let text = std::str::from_utf8(path).map_err(|_| HostError::Ownership)?;
    let (component, tail) = if let Some(tail) = text.strip_prefix("Runner_") {
        (Component::Runner, tail)
    } else if let Some(tail) = text.strip_prefix("Worker_") {
        (Component::Worker, tail)
    } else {
        return Err(HostError::Ownership);
    };
    let timestamp = tail.strip_suffix("-utc.log").ok_or(HostError::Ownership)?;
    if timestamp.len() != 15
        || timestamp.as_bytes()[8] != b'-'
        || !timestamp
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_digit())
    {
        return Err(HostError::Ownership);
    }
    let date = format!(
        "{}-{}-{} {}:{}:{}Z",
        &timestamp[0..4],
        &timestamp[4..6],
        &timestamp[6..8],
        &timestamp[9..11],
        &timestamp[11..13],
        &timestamp[13..15]
    );
    if !valid_timestamp(date.as_bytes()) {
        return Err(HostError::Ownership);
    }
    Ok((component, date))
}

pub(super) fn parse_log_lines(
    bytes: &[u8],
    component: Component,
    events: &mut Vec<Event>,
    unrecognized: &mut u64,
    omitted: &mut u64,
) {
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            *unrecognized = unrecognized.saturating_add(1);
            continue;
        }
        if let Some((timestamp, category)) = parse_trace_header(line) {
            if events.len() < MAX_EVENTS {
                events.push(Event {
                    timestamp,
                    component,
                    category,
                });
            } else {
                *omitted = omitted.saturating_add(1);
            }
        } else {
            *unrecognized = unrecognized.saturating_add(1);
        }
    }
}

fn parse_trace_header(line: &[u8]) -> Option<(String, Category)> {
    if line.len() < 30 || line.first() != Some(&b'[') || line.get(21) != Some(&b' ') {
        return None;
    }
    let timestamp = &line[1..21];
    if !valid_timestamp(timestamp) || line.get(26) != Some(&b' ') {
        return None;
    }
    let category = match &line[22..26] {
        b"CRIT" => Category::Critical,
        b"ERR " => Category::Error,
        b"WARN" => Category::Warning,
        b"INFO" => Category::Information,
        b"VERB" => Category::Verbose,
        _ => return None,
    };
    let closing = line[27..].iter().position(|byte| *byte == b']')? + 27;
    if closing == 27 || line.get(closing + 1) != Some(&b' ') {
        return None;
    }
    let source = &line[27..closing];
    if source.len() > 128
        || !source
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some((String::from_utf8(timestamp.to_vec()).ok()?, category))
}

pub(super) fn valid_timestamp(value: &[u8]) -> bool {
    if value.len() != 20
        || value[4] != b'-'
        || value[7] != b'-'
        || value[10] != b' '
        || value[13] != b':'
        || value[16] != b':'
        || value[19] != b'Z'
    {
        return false;
    }
    if value
        .iter()
        .enumerate()
        .any(|(index, byte)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit())
    {
        return false;
    }
    let number = |start: usize, end: usize| {
        std::str::from_utf8(&value[start..end])
            .ok()?
            .parse::<u32>()
            .ok()
    };
    let Some(year) = number(0, 4) else {
        return false;
    };
    if year == 0 {
        return false;
    }
    let Some(month) = number(5, 7) else {
        return false;
    };
    let Some(day) = number(8, 10) else {
        return false;
    };
    let Some(hour) = number(11, 13) else {
        return false;
    };
    let Some(minute) = number(14, 16) else {
        return false;
    };
    let Some(second) = number(17, 19) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day) && hour < 24 && minute < 60 && second < 60
}
