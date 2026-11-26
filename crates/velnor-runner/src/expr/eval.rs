// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::env::{ExprEnv, Function, JobStatus};
use super::error::ExprError;
use super::format::{evaluate_format, to_json};
use super::hash_files::hash_files;
use super::index::evaluate_index;
use super::node::{BinaryOperator, Node};
use super::parser::parse;
use super::value::{
    ArrayValue, ObjectValue, Value, ordinal_ignore_case_contains, ordinal_ignore_case_ends_with,
    ordinal_ignore_case_starts_with,
};
use crate::context_value::{ContextValue, NonFinite};

pub fn evaluate_expression(source: &str, env: &ExprEnv) -> Result<Value, ExprError> {
    match parse(source, env)? {
        None => Ok(Value::Null),
        Some(node) => evaluate_node(&node, env),
    }
}

pub fn evaluate_condition(source: &str, env: &ExprEnv) -> Result<bool, ExprError> {
    if source.trim().is_empty() {
        return Ok(env.status == JobStatus::Success);
    }
    Ok(evaluate_expression(source, env)?.is_truthy())
}

pub(crate) fn evaluate_node(node: &Node, env: &ExprEnv) -> Result<Value, ExprError> {
    match node {
        Node::Literal(value) => Ok(value.clone()),
        Node::Wildcard => Ok(Value::String("*".to_string())),
        Node::NamedValue(root, _) => Ok(env.context(*root)),
        Node::Not(inner) => Ok(Value::Boolean(evaluate_node(inner, env)?.is_falsy())),
        Node::And(parameters) => {
            let mut result = Value::Null;
            for parameter in parameters {
                result = evaluate_node(parameter, env)?;
                if result.is_falsy() {
                    break;
                }
            }
            Ok(result)
        }
        Node::Or(parameters) => {
            let mut result = Value::Null;
            for parameter in parameters {
                result = evaluate_node(parameter, env)?;
                if result.is_truthy() {
                    break;
                }
            }
            Ok(result)
        }
        Node::Binary(operator, left, right) => {
            let left = evaluate_node(left, env)?;
            let right = evaluate_node(right, env)?;
            Ok(Value::Boolean(match operator {
                BinaryOperator::Equal => left.abstract_equal(&right),
                BinaryOperator::NotEqual => left.abstract_not_equal(&right),
                BinaryOperator::GreaterThan => left.abstract_greater_than(&right),
                BinaryOperator::GreaterThanOrEqual => left.abstract_greater_than_or_equal(&right),
                BinaryOperator::LessThan => left.abstract_less_than(&right),
                BinaryOperator::LessThanOrEqual => left.abstract_less_than_or_equal(&right),
            }))
        }
        Node::Index(target, index) => evaluate_index(node, target, index, env),
        Node::Function { function, args, .. } => evaluate_function(node, *function, args, env),
    }
}

fn arity_error() -> ExprError {
    ExprError::evaluation("Invalid number of function arguments")
}

fn evaluate_function(
    node: &Node,
    function: Function,
    args: &[Node],
    env: &ExprEnv,
) -> Result<Value, ExprError> {
    match function {
        Function::Contains => evaluate_contains(args, env),
        Function::Format => evaluate_format(node, args, env),
        Function::StartsWith => evaluate_affix(args, env, ordinal_ignore_case_starts_with),
        Function::EndsWith => evaluate_affix(args, env, ordinal_ignore_case_ends_with),
        Function::ToJson => {
            let [value] = args else {
                return Err(arity_error());
            };
            to_json(node, &evaluate_node(value, env)?).map(Value::String)
        }
        Function::FromJson => {
            let [value] = args else {
                return Err(arity_error());
            };
            let json = evaluate_node(value, env)?.convert_to_string();
            ContextValue::from_json_str(&json)
                .map(|parsed| from_context_value(&parsed))
                .map_err(|error| ExprError::evaluation(format!("Error parsing fromJson: {error}")))
        }
        Function::HashFiles => {
            let values = args
                .iter()
                .map(|arg| evaluate_node(arg, env))
                .collect::<Result<Vec<_>, _>>()?;
            hash_files(
                &env.workspace,
                env.home.as_deref(),
                &values,
                env.hash_files_timeout,
                &env.walk_seam,
            )
            .map(Value::String)
        }
        Function::Success => Ok(Value::Boolean(env.status == JobStatus::Success)),
        Function::Always => Ok(Value::Boolean(true)),
        Function::Cancelled => Ok(Value::Boolean(env.status == JobStatus::Cancelled)),
        Function::Failure => Ok(Value::Boolean(env.status == JobStatus::Failure)),
    }
}

fn evaluate_contains(args: &[Node], env: &ExprEnv) -> Result<Value, ExprError> {
    let [left, right] = args else {
        return Err(arity_error());
    };
    let left = evaluate_node(left, env)?;
    if left.is_primitive() {
        let right = evaluate_node(right, env)?;
        return Ok(Value::Boolean(
            right.is_primitive()
                && ordinal_ignore_case_contains(
                    &left.convert_to_string(),
                    &right.convert_to_string(),
                ),
        ));
    }
    if let Value::Array(array) = &left
        && !array.items().is_empty()
    {
        let right = evaluate_node(right, env)?;
        return Ok(Value::Boolean(
            array.items().iter().any(|item| right.abstract_equal(item)),
        ));
    }
    Ok(Value::Boolean(false))
}

fn evaluate_affix(
    args: &[Node],
    env: &ExprEnv,
    matches_affix: fn(&str, &str) -> bool,
) -> Result<Value, ExprError> {
    let [left, right] = args else {
        return Err(arity_error());
    };
    let left = evaluate_node(left, env)?;
    if !left.is_primitive() {
        return Ok(Value::Boolean(false));
    }
    let right = evaluate_node(right, env)?;
    Ok(Value::Boolean(
        right.is_primitive()
            && matches_affix(&left.convert_to_string(), &right.convert_to_string()),
    ))
}

pub(crate) fn from_context_value(value: &ContextValue) -> Value {
    match value {
        ContextValue::Null => Value::Null,
        ContextValue::Bool(value) => Value::Boolean(*value),
        ContextValue::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
        ContextValue::BigInteger(value) => Value::Number(value.parse().unwrap_or(f64::NAN)),
        ContextValue::NonFinite(value) => Value::Number(match value {
            NonFinite::NaN => f64::NAN,
            NonFinite::PositiveInfinity => f64::INFINITY,
            NonFinite::NegativeInfinity => f64::NEG_INFINITY,
        }),
        ContextValue::String(value) => Value::String(value.clone()),
        ContextValue::Array(values) => Value::Array(ArrayValue::new(
            values.iter().map(from_context_value).collect(),
        )),
        ContextValue::Object {
            case_sensitive,
            entries,
        } => {
            let entries = entries
                .iter()
                .map(|(name, value)| (name.clone(), from_context_value(value)))
                .collect();
            Value::Object(if *case_sensitive {
                ObjectValue::case_sensitive(entries)
            } else {
                ObjectValue::new(entries)
            })
        }
    }
}
