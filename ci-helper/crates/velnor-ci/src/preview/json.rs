// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;
use std::panic::Location;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

use crate::error::CiError;

pub type Res<T> = Result<T, CiError>;

static NULL: Value = Value::Null;

/// Fails closed. The message names the source line, never a value.
#[track_caller]
pub fn require(ok: bool) -> Res<()> {
    if ok {
        Ok(())
    } else {
        Err(CiError::new(format!(
            "source preview check failed at {}",
            Location::caller()
        )))
    }
}

/// Reads of a JSON value that treat a missing key or a wrong type as `null`, which
/// no check accepts.
pub trait Field {
    fn at(&self, key: &str) -> &Value;
    fn is_pos(&self) -> bool;
    fn is_u(&self, expected: u64) -> bool;
    fn is_s(&self, expected: &str) -> bool;
    fn text(&self) -> &str;
}

impl Field for Value {
    fn at(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }

    fn is_pos(&self) -> bool {
        self.as_u64().is_some_and(|n| n > 0)
    }

    fn is_u(&self, expected: u64) -> bool {
        self.as_u64() == Some(expected)
    }

    fn is_s(&self, expected: &str) -> bool {
        self.as_str() == Some(expected)
    }

    fn text(&self) -> &str {
        self.as_str().unwrap_or("")
    }
}

#[must_use]
pub fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[must_use]
pub fn sha40(value: &Value) -> bool {
    value.as_str().is_some_and(|s| is_hex(s, 40))
}

#[must_use]
pub fn hex64(value: &Value) -> bool {
    value.as_str().is_some_and(|s| is_hex(s, 64))
}

#[must_use]
pub fn is_semver(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

#[must_use]
pub fn digest(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

struct Unique(Value);

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Unique;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Unique, E> {
        Ok(Unique(Value::Bool(v)))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Unique, E> {
        Ok(Unique(Value::Number(v.into())))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Unique, E> {
        Ok(Unique(Value::Number(v.into())))
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Unique, E> {
        Number::from_f64(v)
            .map(|n| Unique(Value::Number(n)))
            .ok_or_else(|| E::custom("number"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Unique, E> {
        Ok(Unique(Value::String(v.to_owned())))
    }

    fn visit_unit<E>(self) -> Result<Unique, E> {
        Ok(Unique(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
        let mut items = Vec::new();
        while let Some(Unique(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Unique(Value::Array(items)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
        let mut items = Map::new();
        while let Some((key, Unique(value))) = map.next_entry::<String, Unique>()? {
            if items.insert(key, value).is_some() {
                return Err(serde::de::Error::custom("duplicate key"));
            }
        }
        Ok(Unique(Value::Object(items)))
    }
}

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

/// Parses JSON, rejecting input over `limit` bytes and any object with a repeated key.
pub fn parse(data: &[u8], limit: usize) -> Res<Value> {
    require(data.len() <= limit)?;
    serde_json::from_slice::<Unique>(data)
        .map(|unique| unique.0)
        .map_err(|_| CiError::new("source preview invalid json"))
}

/// Sorted keys, no spaces, and non-ASCII text written as is: the bytes
/// `json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False)` gives.
#[must_use]
pub fn canonical(value: &Value) -> Vec<u8> {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out.into_bytes()
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.push('{');
            for (index, (key, item)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push(':');
                write_canonical(item, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}
