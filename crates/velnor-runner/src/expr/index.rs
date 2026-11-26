// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::env::ExprEnv;
use super::error::ExprError;
use super::eval::evaluate_node;
use super::format::MemoryCounter;
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

    match collection {
        Collection::Array(array) if array.is_filtered() => filtered_index(&array, key.as_ref()),
        Collection::Object(object) => object_index(&object, key.as_ref()),
        Collection::Array(array) => array_index(&array, key.as_ref()),
    }
}

const POINTER_BYTES: usize = 8;
const MIN_OBJECT_BYTES: usize = 24;

fn projection() -> Result<MemoryCounter, ExprError> {
    let mut memory = MemoryCounter::new("wildcard");
    memory.add_bytes(MIN_OBJECT_BYTES)?;
    Ok(memory)
}

fn project(
    values: impl IntoIterator<Item = Value>,
    memory: &mut MemoryCounter,
    result: &mut Vec<Value>,
) -> Result<(), ExprError> {
    for value in values {
        memory.add_bytes(POINTER_BYTES)?;
        result.push(value);
    }
    Ok(())
}

fn filtered_index(array: &ArrayValue, key: Option<&Value>) -> Result<Value, ExprError> {
    let mut memory = MemoryCounter::new("wildcard");
    let mut result = Vec::new();
    for item in array.items() {
        match (item, key) {
            (Value::Object(nested), None) => project(
                nested.entries().iter().map(|(_, value)| value.clone()),
                &mut memory,
                &mut result,
            )?,
            (Value::Object(nested), Some(key)) => {
                if let Some(value) = string_index(key).and_then(|key| nested.get(&key)) {
                    project([value.clone()], &mut memory, &mut result)?;
                }
            }
            (Value::Array(nested), None) => {
                project(nested.items().iter().cloned(), &mut memory, &mut result)?;
            }
            (Value::Array(nested), Some(key)) => {
                if let Some(value) = integer_index(key).and_then(|index| nested.items().get(index))
                {
                    project([value.clone()], &mut memory, &mut result)?;
                }
            }
            _ => {}
        }
    }
    Ok(Value::Array(ArrayValue::filtered(result)))
}

fn object_index(object: &ObjectValue, key: Option<&Value>) -> Result<Value, ExprError> {
    let Some(key) = key else {
        let mut memory = projection()?;
        let mut values = Vec::new();
        project(
            object.entries().iter().map(|(_, value)| value.clone()),
            &mut memory,
            &mut values,
        )?;
        return Ok(Value::Array(ArrayValue::filtered(values)));
    };
    Ok(string_index(key)
        .and_then(|key| object.get(&key).cloned())
        .unwrap_or(Value::Null))
}

fn array_index(array: &ArrayValue, key: Option<&Value>) -> Result<Value, ExprError> {
    let Some(key) = key else {
        let mut memory = projection()?;
        let mut values = Vec::new();
        project(array.items().iter().cloned(), &mut memory, &mut values)?;
        return Ok(Value::Array(ArrayValue::filtered(values)));
    };
    Ok(integer_index(key)
        .and_then(|index| array.items().get(index).cloned())
        .unwrap_or(Value::Null))
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
