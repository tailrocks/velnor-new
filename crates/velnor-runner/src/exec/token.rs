// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Map, Value as Json};

use super::{DeclaredInputs, InputValue};
use crate::context_value::ordinal_ignore_case_eq;
use crate::expr::{ExprEnv, ExprError, Value, evaluate_expression, evaluate_template};
use crate::job_message::context_data::member;
use crate::job_message::template_token::token_literal;

const LITERAL: i64 = 0;
const MAPPING: i64 = 2;
const BASIC_EXPRESSION: i64 = 3;
const INSERT_EXPRESSION: i64 = 4;
const BOOLEAN: i64 = 5;
const NUMBER: i64 = 6;
const NULL: i64 = 7;

fn invalid(what: &str) -> ExprError {
    ExprError::evaluation(format!("unsupported template token {what}"))
}

fn number_text(number: &serde_json::Number) -> String {
    Value::Number(number.as_f64().unwrap_or(f64::NAN)).convert_to_string()
}

enum Scalar<'a> {
    Template(&'a str),
    Expression(&'a str),
    Plain(String),
}

fn classify(token: &Json) -> Result<Scalar<'_>, ExprError> {
    match token {
        Json::String(text) => Ok(Scalar::Template(text)),
        Json::Bool(value) => Ok(Scalar::Plain(value.to_string())),
        Json::Number(number) => Ok(Scalar::Plain(number_text(number))),
        Json::Null => Ok(Scalar::Plain(String::new())),
        Json::Array(_) => Err(invalid("array")),
        Json::Object(object) => {
            let text = |name: &str| {
                member(object, name)
                    .and_then(Json::as_str)
                    .unwrap_or_default()
            };
            Ok(match tag(object)? {
                LITERAL => Scalar::Template(text("lit")),
                BASIC_EXPRESSION | INSERT_EXPRESSION => Scalar::Expression(text("expr")),
                BOOLEAN => Scalar::Plain(
                    member(object, "bool")
                        .and_then(Json::as_bool)
                        .unwrap_or(false)
                        .to_string(),
                ),
                NUMBER => Scalar::Plain(
                    member(object, "num")
                        .and_then(Json::as_number)
                        .map_or_else(|| "0".to_owned(), number_text),
                ),
                NULL => Scalar::Plain(String::new()),
                other => return Err(invalid(&format!("type {other}"))),
            })
        }
    }
}

pub(super) fn scalar(token: &Json, env: &ExprEnv) -> Result<String, ExprError> {
    match classify(token)? {
        Scalar::Template(text) => evaluate_template(text, env),
        Scalar::Expression(source) => Ok(evaluate_expression(source, env)?.convert_to_string()),
        Scalar::Plain(text) => Ok(text),
    }
}

fn declared_value(token: &Json) -> Result<InputValue, ExprError> {
    Ok(match classify(token)? {
        Scalar::Template(text) if !text.contains("${{") => InputValue::Literal(text.to_owned()),
        Scalar::Plain(text) => InputValue::Literal(text),
        Scalar::Template(_) | Scalar::Expression(_) => InputValue::Deferred,
    })
}

pub(super) fn declared(token: &Json) -> Result<DeclaredInputs, ExprError> {
    if expression_of(token)?.is_some() {
        return Ok(DeclaredInputs::Unknown);
    }
    let mut folded: Vec<(String, InputValue)> = Vec::new();
    for (key, value) in pairs(token)? {
        let InputValue::Literal(name) = declared_value(key)? else {
            return Ok(DeclaredInputs::Unknown);
        };
        let value = declared_value(value)?;
        match folded
            .iter_mut()
            .find(|(known, _)| ordinal_ignore_case_eq(known, &name))
        {
            Some(entry) => entry.1 = value,
            None => folded.push((name, value)),
        }
    }
    Ok(DeclaredInputs::Known(folded.into_iter().collect()))
}

fn tag(object: &Map<String, Json>) -> Result<i64, ExprError> {
    match member(object, "type") {
        None => Ok(LITERAL),
        Some(tag) => tag
            .as_i64()
            .ok_or_else(|| malformed("the token type must be an integer")),
    }
}

fn malformed(what: &str) -> ExprError {
    ExprError::evaluation(format!("Unexpected template token: {what}"))
}

fn entry(pair: &Json) -> Result<(&Json, &Json), ExprError> {
    let pair = pair
        .as_object()
        .ok_or_else(|| malformed("a mapping entry was expected"))?;
    Ok((
        member(pair, "key").ok_or_else(|| malformed("a mapping entry has no key"))?,
        member(pair, "value").ok_or_else(|| malformed("a mapping entry has no value"))?,
    ))
}

pub(super) fn pairs(token: &Json) -> Result<Vec<(&Json, &Json)>, ExprError> {
    let object = match token {
        Json::Null => return Ok(Vec::new()),
        Json::Object(object) => object,
        _ => return Err(malformed("a mapping was expected")),
    };
    match tag(object)? {
        NULL => Ok(Vec::new()),
        MAPPING => member(object, "map").map_or_else(
            || Ok(Vec::new()),
            |entries| {
                entries
                    .as_array()
                    .ok_or_else(|| malformed("a mapping was expected"))?
                    .iter()
                    .map(entry)
                    .collect()
            },
        ),
        _ => Err(malformed("a mapping was expected")),
    }
}

fn expression_of(token: &Json) -> Result<Option<&str>, ExprError> {
    let Json::Object(object) = token else {
        return Ok(None);
    };
    Ok(match tag(object)? {
        BASIC_EXPRESSION | INSERT_EXPRESSION => Some(
            member(object, "expr")
                .and_then(Json::as_str)
                .unwrap_or_default(),
        ),
        _ => None,
    })
}

fn assert_string(value: &Value) -> Result<String, ExprError> {
    match value {
        Value::Array(_) | Value::Object(_) => Err(malformed("a string was expected")),
        scalar => Ok(scalar.convert_to_string()),
    }
}

fn evaluated_mapping(expression: &str, env: &ExprEnv) -> Result<Vec<(String, String)>, ExprError> {
    let Value::Object(object) = evaluate_expression(expression, env)? else {
        return Err(malformed("a mapping was expected"));
    };
    object
        .entries()
        .iter()
        .map(|(key, value)| Ok((key.clone(), assert_string(value)?)))
        .collect()
}

pub(super) fn find<'a>(token: &'a Json, name: &str) -> Result<Option<&'a Json>, ExprError> {
    Ok(pairs(token)?
        .into_iter()
        .find(|(key, _)| token_literal(key).is_some_and(|key| key.eq_ignore_ascii_case(name)))
        .map(|(_, value)| value))
}

pub(super) fn mapping(token: &Json, env: &ExprEnv) -> Result<Vec<(String, String)>, ExprError> {
    match expression_of(token)? {
        Some(expression) => evaluated_mapping(expression, env),
        None => pairs(token)?
            .into_iter()
            .map(|(key, value)| Ok((scalar(key, env)?, scalar(value, env)?)))
            .collect(),
    }
}

pub(super) fn input(
    inputs: Option<&Json>,
    name: &str,
    env: &ExprEnv,
) -> Result<Option<String>, ExprError> {
    let Some(inputs) = inputs else {
        return Ok(None);
    };
    if expression_of(inputs)?.is_some() {
        return Ok(mapping(inputs, env)?
            .into_iter()
            .find(|(key, _)| ordinal_ignore_case_eq(key, name))
            .map(|(_, value)| value));
    }
    find(inputs, name)?
        .map(|token| scalar(token, env))
        .transpose()
}

pub(super) fn boolean(token: &Json, env: &ExprEnv) -> Result<bool, ExprError> {
    match token {
        Json::Bool(value) => Ok(*value),
        Json::Null => Ok(false),
        Json::Object(object) => match member(object, "type").and_then(Json::as_i64) {
            Some(BOOLEAN) => Ok(member(object, "bool")
                .and_then(Json::as_bool)
                .unwrap_or(false)),
            Some(NULL) => Ok(false),
            Some(BASIC_EXPRESSION | INSERT_EXPRESSION) => {
                match evaluate_expression(
                    member(object, "expr")
                        .and_then(Json::as_str)
                        .unwrap_or_default(),
                    env,
                )? {
                    Value::Boolean(value) => Ok(value),
                    other => Err(ExprError::evaluation(format!(
                        "Unexpected value '{}': a Boolean was expected",
                        other.convert_to_string()
                    ))),
                }
            }
            _ => Err(not_boolean()),
        },
        _ => Err(not_boolean()),
    }
}

fn not_boolean() -> ExprError {
    ExprError::evaluation("Unexpected type: a Boolean was expected")
}
