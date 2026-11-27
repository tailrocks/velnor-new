// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/../velnor-model/src/context_value.rs

use crate::runner_json::ordered::{OrderedJsonValue, parse_ordered_json};
use serde_json::{Number, Value as JsonValue};
use std::cmp::Ordering;

const MAX_BIG_INTEGER_CHARS: usize = 380;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonFinite {
    NaN,
    PositiveInfinity,
    NegativeInfinity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextValue {
    Null,
    Bool(bool),
    Number(Number),
    BigInteger(String),
    NonFinite(NonFinite),
    String(String),
    Array(Vec<ContextValue>),
    Object {
        case_sensitive: bool,
        entries: Vec<(String, ContextValue)>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContextValueError {
    #[error("context object contains duplicate keys under its comparer")]
    DuplicateObjectKey,
    #[error(
        "big integer must be a canonical decimal outside the i64 range and at most {MAX_BIG_INTEGER_CHARS} characters"
    )]
    InvalidBigInteger,
}

impl ContextValue {
    pub fn big_integer(value: impl Into<String>) -> Result<Self, ContextValueError> {
        let value = value.into();
        validate_big_integer(&value)?;
        Ok(Self::BigInteger(value))
    }

    #[must_use]
    pub const fn non_finite(value: NonFinite) -> Self {
        Self::NonFinite(value)
    }

    pub fn object(entries: Vec<(String, Self)>) -> Result<Self, ContextValueError> {
        Self::try_object(false, entries)
    }

    pub fn case_sensitive_object(entries: Vec<(String, Self)>) -> Result<Self, ContextValueError> {
        Self::try_object(true, entries)
    }

    fn try_object(
        case_sensitive: bool,
        entries: Vec<(String, Self)>,
    ) -> Result<Self, ContextValueError> {
        validate_unique_keys(case_sensitive, &entries)?;
        Ok(Self::Object {
            case_sensitive,
            entries,
        })
    }

    pub fn validate(&self) -> Result<(), ContextValueError> {
        match self {
            Self::Array(values) => values.iter().try_for_each(Self::validate),
            Self::Object {
                case_sensitive,
                entries,
            } => {
                validate_unique_keys(*case_sensitive, entries)?;
                entries.iter().try_for_each(|(_, value)| value.validate())
            }
            Self::BigInteger(value) => validate_big_integer(value),
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::NonFinite(_) | Self::String(_) => {
                Ok(())
            }
        }
    }

    pub fn from_json(value: JsonValue) -> Result<Self, ContextValueError> {
        Self::from_json_with(value, false)
    }

    pub fn from_json_case_sensitive(value: JsonValue) -> Result<Self, ContextValueError> {
        Self::from_json_with(value, true)
    }

    fn from_json_with(value: JsonValue, case_sensitive: bool) -> Result<Self, ContextValueError> {
        match value {
            JsonValue::Null => Ok(Self::Null),
            JsonValue::Bool(value) => Ok(Self::Bool(value)),
            JsonValue::Number(value) => Self::from_json_number(value),
            JsonValue::String(value) => Ok(Self::String(value)),
            JsonValue::Array(values) => values
                .into_iter()
                .map(|value| Self::from_json_with(value, case_sensitive))
                .collect::<Result<_, _>>()
                .map(Self::Array),
            JsonValue::Object(values) => Self::try_object(
                case_sensitive,
                values
                    .into_iter()
                    .map(|(key, value)| {
                        Self::from_json_with(value, case_sensitive).map(|value| (key, value))
                    })
                    .collect::<Result<_, _>>()?,
            ),
        }
    }

    fn from_json_number(value: Number) -> Result<Self, ContextValueError> {
        match value.as_u64() {
            Some(unsigned) if i64::try_from(unsigned).is_err() => {
                Self::big_integer(unsigned.to_string())
            }
            _ => Ok(Self::Number(value)),
        }
    }

    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        Self::from_json_text(json, false)
    }

    pub fn from_json_str_case_sensitive(json: &str) -> Result<Self, serde_json::Error> {
        Self::from_json_text(json, true)
    }

    fn from_json_text(json: &str, case_sensitive: bool) -> Result<Self, serde_json::Error> {
        Self::from_ordered(parse_ordered_json(json)?, case_sensitive)
            .map_err(<serde_json::Error as serde::de::Error>::custom)
    }

    pub fn from_ordered_property_bag(value: OrderedJsonValue) -> Result<Self, ContextValueError> {
        match value {
            OrderedJsonValue::Object(entries) => Self::try_object(
                false,
                entries
                    .into_iter()
                    .map(|(key, value)| Self::from_ordered(value, true).map(|value| (key, value)))
                    .collect::<Result<_, _>>()?,
            ),
            other => Self::from_ordered(other, true),
        }
    }

    pub fn from_ordered(
        value: OrderedJsonValue,
        case_sensitive: bool,
    ) -> Result<Self, ContextValueError> {
        match value {
            OrderedJsonValue::Null => Ok(Self::Null),
            OrderedJsonValue::Bool(value) => Ok(Self::Bool(value)),
            OrderedJsonValue::Number(value) => Self::from_json_number(value),
            OrderedJsonValue::String(value) => Ok(Self::String(value)),
            OrderedJsonValue::Array(values) => values
                .into_iter()
                .map(|value| Self::from_ordered(value, case_sensitive))
                .collect::<Result<_, _>>()
                .map(Self::Array),
            OrderedJsonValue::Object(entries) => Self::try_object(
                case_sensitive,
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        Self::from_ordered(value, case_sensitive).map(|value| (key, value))
                    })
                    .collect::<Result<_, _>>()?,
            ),
        }
    }

    #[must_use]
    pub fn entries(&self) -> Option<&[(String, Self)]> {
        match self {
            Self::Object { entries, .. } => Some(entries),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_case_sensitive(&self) -> Option<bool> {
        match self {
            Self::Object { case_sensitive, .. } => Some(*case_sensitive),
            _ => None,
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        let Self::Object {
            case_sensitive,
            entries,
        } = self
        else {
            return None;
        };
        entries
            .iter()
            .find(|(name, _)| keys_equal(*case_sensitive, name, key))
            .map(|(_, value)| value)
    }

    pub fn to_github_json(&self) -> Result<String, ContextValueError> {
        self.validate()?;
        let mut output = String::new();
        self.write_github_json(&mut output);
        Ok(output)
    }

    fn write_github_json(&self, output: &mut String) {
        match self {
            Self::Null => output.push_str("null"),
            Self::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Number(value) => output.push_str(&value.to_string()),
            Self::BigInteger(value) => output.push_str(value),
            Self::NonFinite(value) => write_json_string(non_finite_text(*value), output),
            Self::String(value) => write_json_string(value, output),
            Self::Array(values) => {
                output.push('[');
                for (index, item) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    item.write_github_json(output);
                }
                output.push(']');
            }
            Self::Object { entries, .. } => {
                output.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write_json_string(key, output);
                    output.push(':');
                    value.write_github_json(output);
                }
                output.push('}');
            }
        }
    }
}

#[must_use]
pub const fn non_finite_text(value: NonFinite) -> &'static str {
    match value {
        NonFinite::NaN => "NaN",
        NonFinite::PositiveInfinity => "Infinity",
        NonFinite::NegativeInfinity => "-Infinity",
    }
}

pub(crate) fn write_json_string(value: &str, output: &mut String) {
    use std::fmt::Write as _;

    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{0085}' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(output, "\\u{:04x}", u32::from(character));
            }
            character if character <= '\u{1f}' => {
                let _ = write!(output, "\\u{:04x}", u32::from(character));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

fn validate_big_integer(value: &str) -> Result<(), ContextValueError> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if value.len() > MAX_BIG_INTEGER_CHARS
        || digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || value.parse::<i64>().is_ok()
    {
        return Err(ContextValueError::InvalidBigInteger);
    }
    Ok(())
}

fn keys_equal(case_sensitive: bool, left: &str, right: &str) -> bool {
    if case_sensitive {
        left == right
    } else {
        ordinal_ignore_case_eq(left, right)
    }
}

fn validate_unique_keys(
    case_sensitive: bool,
    entries: &[(String, ContextValue)],
) -> Result<(), ContextValueError> {
    for (index, (key, _)) in entries.iter().enumerate() {
        if entries[..index]
            .iter()
            .any(|(previous, _)| keys_equal(case_sensitive, previous, key))
        {
            return Err(ContextValueError::DuplicateObjectKey);
        }
    }
    Ok(())
}

#[must_use]
pub fn ordinal_ignore_case_eq(left: &str, right: &str) -> bool {
    ordinal_ignore_case_cmp(left, right) == Ordering::Equal
}

#[must_use]
pub fn ordinal_ignore_case_cmp(left: &str, right: &str) -> Ordering {
    left.chars()
        .map(simple_ordinal_uppercase)
        .cmp(right.chars().map(simple_ordinal_uppercase))
}

pub(crate) fn ordinal_ignore_case_fold(value: &str) -> impl Iterator<Item = char> + '_ {
    value.chars().map(simple_ordinal_uppercase)
}

const YPOGEGRAMMENI_TO_PROSGEGRAMMENI: u32 = 8;

fn simple_ordinal_uppercase(character: char) -> char {
    match character {
        '\u{0131}' | '\u{017f}' => character,
        '\u{1f80}'..='\u{1f87}' | '\u{1f90}'..='\u{1f97}' | '\u{1fa0}'..='\u{1fa7}' => {
            char::from_u32(u32::from(character) + YPOGEGRAMMENI_TO_PROSGEGRAMMENI)
                .unwrap_or(character)
        }
        '\u{1fb3}' => '\u{1fbc}',
        '\u{1fc3}' => '\u{1fcc}',
        '\u{1ff3}' => '\u{1ffc}',
        _ => {
            let mut upper = character.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => single,
                _ => character,
            }
        }
    }
}
