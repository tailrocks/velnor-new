// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::CiError;

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

/// Text for a workflow-command message or any stdout line the runner reads. Line breaks and `%`
/// are encoded, so a value cannot end its line or start a command of its own.
#[must_use]
pub fn escape_data(text: &str) -> String {
    escape(text, false)
}

/// Text for a workflow-command property such as `file=`. Also encodes `:` and `,`.
#[must_use]
pub fn escape_property(text: &str) -> String {
    escape(text, true)
}

/// A value for `GITHUB_OUTPUT`, `GITHUB_ENV` or `GITHUB_PATH`. The runner ends the value at a line
/// break, so a value holding `\r` or `\n` would start a line of its own: a key, or a PATH entry.
/// Such a value is refused, and the step fails, rather than written.
pub fn file_value(value: &str) -> Result<&str, CiError> {
    if value.contains(['\r', '\n']) {
        return Err(CiError::new(
            "a value for GITHUB_OUTPUT, GITHUB_ENV or GITHUB_PATH holds a line break; refused",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{escape_data, escape_property, file_value};

    #[test]
    fn data_value_with_a_line_break_stays_on_one_line() {
        let text = escape_data("cache\n::stop-commands::x\r::error::forged");
        assert_eq!(text, "cache%0A::stop-commands::x%0D::error::forged");
        assert!(!text.contains(['\r', '\n']));
    }

    #[test]
    fn property_value_also_encodes_separators() {
        assert_eq!(escape_property("a:b,c\n"), "a%3Ab%2Cc%0A");
    }

    #[test]
    fn file_value_refuses_a_line_break_that_would_add_a_key() {
        assert_eq!(file_value("/runner/temp").ok(), Some("/runner/temp"));
        assert!(file_value("x\nowner=evil").is_err());
        assert!(file_value("x\rowner=evil").is_err());
    }
}
