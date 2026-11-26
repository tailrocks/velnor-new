// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const SHORT_SECTION: usize = 6;
const QUOTED_MINIMUM: usize = 8;

pub(super) const ENCODERS: [fn(&str) -> String; 11] = [
    |value| base64_shifted(value, 0),
    |value| base64_shifted(value, 1),
    |value| base64_shifted(value, 2),
    command_line,
    expression,
    json,
    uri_data,
    xml,
    trim_double_quotes,
    before_ampersand,
    after_ampersand,
];

fn base64_shifted(value: &str, shift: usize) -> String {
    let bytes = value.as_bytes();
    let shifted = bytes.get(shift..).filter(|rest| !rest.is_empty());
    STANDARD.encode(shifted.unwrap_or(bytes))
}

fn command_line(value: &str) -> String {
    value.replace('"', "\\\"")
}

fn expression(value: &str) -> String {
    value.replace('\'', "''")
}

fn json(value: &str) -> String {
    let quoted = serde_json::Value::String(value.to_owned()).to_string();
    quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default()
        .to_owned()
}

fn uri_data(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn trim_double_quotes(value: &str) -> String {
    if value.chars().count() > QUOTED_MINIMUM {
        value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .map_or_else(String::new, str::to_owned)
    } else {
        String::new()
    }
}

fn section(value: &str, before: bool) -> String {
    let part = match (value.find("&+"), value.rfind('&')) {
        (Some(at), _) => {
            let end = at + "&+".len();
            if before {
                Some(&value[..end])
            } else {
                value[end..]
                    .chars()
                    .next()
                    .map(|skipped| &value[end + skipped.len_utf8()..])
            }
        }
        (None, Some(at)) if before => Some(&value[..=at]),
        (None, Some(at)) => Some(&value[at + 1..]),
        (None, None) => None,
    };
    part.filter(|part| part.chars().count() >= SHORT_SECTION)
        .map_or_else(String::new, str::to_owned)
}

fn before_ampersand(value: &str) -> String {
    section(value, true)
}

fn after_ampersand(value: &str) -> String {
    section(value, false)
}
