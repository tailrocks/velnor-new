// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::env::ExprEnv;
use super::error::ExprError;
use super::eval::evaluate_node;
use super::node::Node;
use super::value::{ArrayValue, ObjectValue, Value};

enum Collection {
    Array(ArrayValue),
    Object(ObjectValue),
}

pub(crate) fn evaluate_index(
    target: &Node,
    index: &Node,
    env: &ExprEnv,
) -> Result<Value, ExprError> {
    let left = evaluate_node(target, env)?;
    let is_wildcard = matches!(index, Node::Wildcard);

    let collection = match left {
        Value::Array(array) => Collection::Array(array),
        Value::Object(object) => Collection::Object(object),
        _ => {
            return Ok(if is_wildcard {
                Value::Array(ArrayValue::filtered(Vec::new()))
            } else {
                Value::Null
            });
        }
    };

    let key = if is_wildcard {
        None
    } else {
        Some(evaluate_node(index, env)?)
    };

    Ok(match collection {
        Collection::Array(array) if array.is_filtered() => filtered_index(&array, key.as_ref()),
        Collection::Object(object) => object_index(&object, key.as_ref()),
        Collection::Array(array) => array_index(&array, key.as_ref()),
    })
}

fn filtered_index(array: &ArrayValue, key: Option<&Value>) -> Value {
    let mut result = Vec::new();
    for item in array.items() {
        match (item, key) {
            (Value::Object(nested), None) => {
                result.extend(nested.entries().iter().map(|(_, value)| value.clone()));
            }
            (Value::Object(nested), Some(key)) => {
                if let Some(value) = string_index(key).and_then(|key| nested.get(&key)) {
                    result.push(value.clone());
                }
            }
            (Value::Array(nested), None) => result.extend(nested.items().iter().cloned()),
            (Value::Array(nested), Some(key)) => {
                if let Some(value) = integer_index(key).and_then(|index| nested.items().get(index))
                {
                    result.push(value.clone());
                }
            }
            _ => {}
        }
    }
    Value::Array(ArrayValue::filtered(result))
}

fn object_index(object: &ObjectValue, key: Option<&Value>) -> Value {
    let Some(key) = key else {
        let values = object
            .entries()
            .iter()
            .map(|(_, value)| value.clone())
            .collect();
        return Value::Array(ArrayValue::filtered(values));
    };
    string_index(key)
        .and_then(|key| object.get(&key).cloned())
        .unwrap_or(Value::Null)
}

fn array_index(array: &ArrayValue, key: Option<&Value>) -> Value {
    let Some(key) = key else {
        return Value::Array(ArrayValue::filtered(array.items().to_vec()));
    };
    integer_index(key)
        .and_then(|index| array.items().get(index).cloned())
        .unwrap_or(Value::Null)
}

fn string_index(value: &Value) -> Option<String> {
    value.is_primitive().then(|| value.convert_to_string())
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is checked to be a non-negative integer no greater than i32::MAX"
)]
fn integer_index(value: &Value) -> Option<usize> {
    let index = value.convert_to_number();
    if index.is_nan() || index < 0.0 {
        return None;
    }
    let index = index.floor();
    (index <= f64::from(i32::MAX)).then_some(index as usize)
}
