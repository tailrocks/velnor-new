// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::error::CiError;

struct Strict(Value);

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Strict;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Strict, E> {
        Ok(Strict(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Strict, E> {
        Ok(Strict(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Strict, E> {
        Ok(Strict(Value::Number(value.into())))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Strict, E> {
        Number::from_f64(value)
            .map(|number| Strict(Value::Number(number)))
            .ok_or_else(|| E::custom("number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Strict, E> {
        Ok(Strict(Value::String(value.to_owned())))
    }

    fn visit_unit<E>(self) -> Result<Strict, E> {
        Ok(Strict(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Strict, A::Error> {
        let mut items = Vec::new();
        while let Some(Strict(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Strict(Value::Array(items)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Strict, A::Error> {
        let mut object = Map::new();
        while let Some((key, Strict(value))) = map.next_entry::<String, Strict>()? {
            if object.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("Duplicate JSON key: {key}")));
            }
        }
        Ok(Strict(Value::Object(object)))
    }
}

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor)
    }
}

pub fn parse(data: &[u8], label: &str) -> Result<Value, CiError> {
    serde_json::from_slice::<Strict>(data)
        .map(|strict| strict.0)
        .map_err(|error| {
            let message = error.to_string();
            match message.split(" at line ").next() {
                Some(text) if text.starts_with("Duplicate JSON key") => CiError::new(text),
                _ => constant_before(data, &error).map_or_else(
                    || CiError::new(format!("{label} JSON has an incorrect format.")),
                    |name| CiError::new(format!("JSON constant has an incorrect value: {name}.")),
                ),
            }
        })
}

fn constant_before(data: &[u8], error: &serde_json::Error) -> Option<&'static str> {
    let line_start = data
        .split(|byte| *byte == b'\n')
        .take(error.line().saturating_sub(1))
        .map(|line| line.len() + 1)
        .sum::<usize>();
    let limit = (line_start + error.column()).min(data.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in data[..limit].iter().enumerate() {
        if in_string {
            match (escaped, *byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
        } else if *byte == b'"' {
            in_string = true;
        } else {
            for name in ["NaN", "Infinity", "-Infinity"] {
                if data[index..].starts_with(name.as_bytes()) {
                    return Some(name);
                }
            }
        }
    }
    None
}

#[must_use]
pub fn uint(value: &Value) -> Option<u64> {
    value.as_u64()
}

#[must_use]
pub fn is_int_at_least(value: &Value, least: u64) -> bool {
    uint(value).is_some_and(|number| number >= least)
}

#[must_use]
pub fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[must_use]
pub fn is_digest(value: &Value) -> bool {
    value
        .as_str()
        .and_then(|text| text.strip_prefix("sha256:"))
        .is_some_and(|hex| is_hex(hex, 64))
}
