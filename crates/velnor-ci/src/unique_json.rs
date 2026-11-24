// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A JSON parse that rejects an object with a repeated key, which `serde_json::Value`
//! alone keeps silently (the last value wins).

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::error::CiError;

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

/// Parses JSON, rejecting any object with a repeated key.
pub fn parse(data: &[u8]) -> Result<Value, CiError> {
    serde_json::from_slice::<Unique>(data)
        .map(|unique| unique.0)
        .map_err(|_| CiError::new("invalid json"))
}
