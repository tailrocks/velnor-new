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
    use super::file_value;

    #[test]
    fn file_value_refuses_a_line_break_that_would_add_a_key() {
        assert_eq!(file_value("/runner/temp").ok(), Some("/runner/temp"));
        assert!(file_value("x\nowner=evil").is_err());
        assert!(file_value("x\rowner=evil").is_err());
    }
}
