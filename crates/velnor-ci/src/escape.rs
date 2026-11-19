// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub fn inert(text: &str) -> String {
    escape(text, false).replace(':', "%3A")
}

#[must_use]
pub fn escape(text: &str, property: bool) -> String {
    text.chars()
        .map(|c| match c {
            '%' => "%25".to_owned(),
            '\r' => "%0D".to_owned(),
            '\n' => "%0A".to_owned(),
            ':' if property => "%3A".to_owned(),
            ',' if property => "%2C".to_owned(),
            odd if odd.is_control() || (odd.is_whitespace() && odd != ' ') => {
                format!("%{:02X}", u32::from(odd))
            }
            other => other.to_string(),
        })
        .collect::<String>()
        .replace("##[", "#%23[")
}

#[must_use]
pub fn stderr_line(message: &str) -> String {
    format!("velnor-ci: {}", escape(message, false))
}
