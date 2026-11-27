// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::value::{ArrayValue, ObjectValue, Value};
use crate::context_value::ordinal_ignore_case_eq;
use crate::runner_json::ordered::{OrderedJsonValue, parse_ordered_json};

const MAX_DEPTH: usize = 64;

pub(super) fn parse_from_json(json: &str) -> Result<Value, String> {
    let parsed = parse_ordered_json(json).map_err(|error| error.to_string())?;
    convert(parsed, 0)
}

fn convert(value: OrderedJsonValue, depth: usize) -> Result<Value, String> {
    match value {
        OrderedJsonValue::Null => Ok(Value::Null),
        OrderedJsonValue::Bool(value) => Ok(Value::Boolean(value)),
        OrderedJsonValue::Number(value) => Ok(Value::Number(value.as_f64().unwrap_or(f64::NAN))),
        OrderedJsonValue::String(value) => Ok(Value::String(value)),
        OrderedJsonValue::Array(items) => {
            let depth = enter(depth)?;
            items
                .into_iter()
                .map(|item| convert(item, depth))
                .collect::<Result<_, _>>()
                .map(|items| Value::Array(ArrayValue::new(items)))
        }
        OrderedJsonValue::Object(entries) => {
            let depth = enter(depth)?;
            let mut merged: Vec<(String, Value)> = Vec::with_capacity(entries.len());
            for (name, item) in entries {
                let item = convert(item, depth)?;
                match merged
                    .iter_mut()
                    .find(|(existing, _)| ordinal_ignore_case_eq(existing, &name))
                {
                    Some((_, slot)) => *slot = item,
                    None => merged.push((name, item)),
                }
            }
            Ok(Value::Object(ObjectValue::new(merged)))
        }
    }
}

fn enter(depth: usize) -> Result<usize, String> {
    if depth >= MAX_DEPTH {
        return Err(format!(
            "The reader's MaxDepth of {MAX_DEPTH} has been exceeded."
        ));
    }
    Ok(depth + 1)
}
