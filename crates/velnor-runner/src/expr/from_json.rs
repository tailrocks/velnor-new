// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::from_json_depth::exceeded;
use super::value::{ArrayValue, ObjectValue, Value};
use crate::context_value::ordinal_ignore_case_eq;
use crate::runner_json::ordered::{OrderedJsonValue, parse_ordered_json};

pub(super) fn parse_from_json(json: &str) -> Result<Value, String> {
    if let Some(exceeded) = exceeded(json) {
        return Err(match parse_ordered_json(&exceeded.probe) {
            Ok(_) => exceeded.message,
            Err(error) => syntax_error(&error),
        });
    }
    parse_ordered_json(json)
        .map(convert)
        .map_err(|error| syntax_error(&error))
}

fn syntax_error(error: &serde_json::Error) -> String {
    format!("Error parsing fromJson: {error}")
}

fn convert(value: OrderedJsonValue) -> Value {
    match value {
        OrderedJsonValue::Null => Value::Null,
        OrderedJsonValue::Bool(value) => Value::Boolean(value),
        OrderedJsonValue::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
        OrderedJsonValue::String(value) => Value::String(value),
        OrderedJsonValue::Array(items) => {
            Value::Array(ArrayValue::new(items.into_iter().map(convert).collect()))
        }
        OrderedJsonValue::Object(entries) => {
            let converted = upsert_all(entries, |left, right| left == right)
                .into_iter()
                .map(|(name, item)| (name, convert(item)))
                .collect();
            Value::Object(ObjectValue::new(upsert_all(
                converted,
                ordinal_ignore_case_eq,
            )))
        }
    }
}

fn upsert_all<V>(entries: Vec<(String, V)>, same: impl Fn(&str, &str) -> bool) -> Vec<(String, V)> {
    let mut merged: Vec<(String, V)> = Vec::with_capacity(entries.len());
    for (name, item) in entries {
        match merged
            .iter_mut()
            .find(|(existing, _)| same(existing, &name))
        {
            Some((_, slot)) => *slot = item,
            None => merged.push((name, item)),
        }
    }
    merged
}
