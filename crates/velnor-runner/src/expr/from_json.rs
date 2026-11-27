// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::from_json_depth::exceeded;
use super::from_json_number::split_overflow;
use super::value::{ArrayValue, ObjectValue, Value};
use crate::context_value::ordinal_ignore_case_fold;
use crate::runner_json::ordered::{OrderedJsonValue, parse_ordered_json};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;

pub(super) fn parse_from_json(json: &str) -> Result<Value, String> {
    let split = split_overflow(json);
    let primary = split.as_ref().map_or(json, |split| split.primary.as_str());
    if let Some(exceeded) = exceeded(primary) {
        return Err(match parse_ordered_json(&exceeded.probe) {
            Ok(_) => exceeded.message,
            Err(error) => syntax_error(&error),
        });
    }
    let value = parse_ordered_json(primary).map_err(|error| syntax_error(&error))?;
    let twin = split
        .map(|split| parse_ordered_json(&split.twin))
        .transpose()
        .map_err(|error| syntax_error(&error))?;
    Ok(convert(value, twin))
}

fn syntax_error(error: &serde_json::Error) -> String {
    format!("Error parsing fromJson: {error}")
}

fn convert(value: OrderedJsonValue, twin: Option<OrderedJsonValue>) -> Value {
    match value {
        OrderedJsonValue::Null => Value::Null,
        OrderedJsonValue::Bool(value) => Value::Boolean(value),
        OrderedJsonValue::Number(value) => match twin {
            Some(OrderedJsonValue::Number(other)) if other != value => {
                Value::Number(if value.as_f64().is_some_and(f64::is_sign_negative) {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                })
            }
            _ => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
        },
        OrderedJsonValue::String(value) => Value::String(value),
        OrderedJsonValue::Array(items) => {
            let mut twins = match twin {
                Some(OrderedJsonValue::Array(twins)) => twins,
                _ => Vec::new(),
            }
            .into_iter();
            Value::Array(ArrayValue::new(
                items
                    .into_iter()
                    .map(|item| convert(item, twins.next()))
                    .collect(),
            ))
        }
        OrderedJsonValue::Object(entries) => {
            let mut twins = match twin {
                Some(OrderedJsonValue::Object(twins)) => twins,
                _ => Vec::new(),
            }
            .into_iter();
            let paired = entries
                .into_iter()
                .map(|(name, item)| (name, (item, twins.next().map(|(_, twin)| twin))))
                .collect();
            let converted = merge(paired, str::to_owned)
                .into_iter()
                .map(|(name, (item, twin))| (name, convert(item, twin)))
                .collect();
            Value::Object(ObjectValue::new(merge(converted, |name| {
                ordinal_ignore_case_fold(name).collect::<String>()
            })))
        }
    }
}

fn merge<V, K: Hash + Eq>(entries: Vec<(String, V)>, key: impl Fn(&str) -> K) -> Vec<(String, V)> {
    let mut slots: HashMap<K, usize> = HashMap::with_capacity(entries.len());
    let mut merged: Vec<(String, V)> = Vec::with_capacity(entries.len());
    for (name, item) in entries {
        match slots.entry(key(&name)) {
            Entry::Occupied(slot) => merged[*slot.get()].1 = item,
            Entry::Vacant(slot) => {
                slot.insert(merged.len());
                merged.push((name, item));
            }
        }
    }
    merged
}
