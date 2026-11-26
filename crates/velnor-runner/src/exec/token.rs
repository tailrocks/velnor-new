// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value as Json;

use crate::expr::{ExprEnv, ExprError, Value, evaluate_expression, evaluate_template};
use crate::job_message::context_data::member;
use crate::job_message::template_token::token_literal;

const LITERAL: i64 = 0;
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

pub(super) fn scalar(token: &Json, env: &ExprEnv) -> Result<String, ExprError> {
    match token {
        Json::String(text) => evaluate_template(text, env),
        Json::Bool(value) => Ok(value.to_string()),
        Json::Number(number) => Ok(number_text(number)),
        Json::Null => Ok(String::new()),
        Json::Array(_) => Err(invalid("array")),
        Json::Object(object) => {
            let text = |name: &str| member(object, name).and_then(Json::as_str);
            match member(object, "type")
                .and_then(Json::as_i64)
                .unwrap_or(LITERAL)
            {
                LITERAL => evaluate_template(text("lit").unwrap_or_default(), env),
                BASIC_EXPRESSION | INSERT_EXPRESSION => {
                    Ok(evaluate_expression(text("expr").unwrap_or_default(), env)?
                        .convert_to_string())
                }
                BOOLEAN => Ok(member(object, "bool")
                    .and_then(Json::as_bool)
                    .unwrap_or(false)
                    .to_string()),
                NUMBER => Ok(member(object, "num")
                    .and_then(Json::as_number)
                    .map_or_else(|| "0".to_owned(), number_text)),
                NULL => Ok(String::new()),
                other => Err(invalid(&format!("type {other}"))),
            }
        }
    }
}

pub(super) fn pairs(token: &Json) -> Vec<(&Json, &Json)> {
    let Some(entries) = token
        .as_object()
        .and_then(|object| member(object, "map"))
        .and_then(Json::as_array)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(Json::as_object)
        .filter_map(|pair| Some((member(pair, "key")?, member(pair, "value")?)))
        .collect()
}

pub(super) fn find<'a>(token: &'a Json, name: &str) -> Option<&'a Json> {
    pairs(token)
        .into_iter()
        .find(|(key, _)| token_literal(key).is_some_and(|key| key.eq_ignore_ascii_case(name)))
        .map(|(_, value)| value)
}

pub(super) fn mapping(token: &Json, env: &ExprEnv) -> Result<Vec<(String, String)>, ExprError> {
    pairs(token)
        .into_iter()
        .map(|(key, value)| Ok((scalar(key, env)?, scalar(value, env)?)))
        .collect()
}

pub(super) fn input(
    inputs: Option<&Json>,
    name: &str,
    env: &ExprEnv,
) -> Result<Option<String>, ExprError> {
    inputs
        .and_then(|inputs| find(inputs, name))
        .map(|token| scalar(token, env))
        .transpose()
}
