// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use anyhow::{Context, Result, bail};
use serde_json::Value;

use super::context_data::member;
use crate::context_value::{ContextValue, NonFinite};

const STRING: i64 = 0;
const SEQUENCE: i64 = 1;
const MAPPING: i64 = 2;
const BASIC_EXPRESSION: i64 = 3;
const INSERT_EXPRESSION: i64 = 4;
const BOOLEAN: i64 = 5;
const NUMBER: i64 = 6;
const NULL: i64 = 7;

pub(crate) fn token_literal(token: &Value) -> Option<&str> {
    match token {
        Value::String(text) => Some(text),
        Value::Object(object) => member(object, "lit")?.as_str(),
        _ => None,
    }
}

pub fn template_token_context_value(token: &Value) -> Result<ContextValue> {
    let Value::Object(object) = token else {
        return if token.is_array() {
            Ok(ContextValue::Null)
        } else {
            ContextValue::from_json(token.clone()).map_err(anyhow::Error::from)
        };
    };
    let kind = member(object, "type")
        .and_then(Value::as_i64)
        .unwrap_or(STRING);
    match kind {
        STRING => Ok(text_member(object, "lit")),
        SEQUENCE => sequence(object),
        MAPPING => mapping(object),
        BASIC_EXPRESSION => Ok(text_member(object, "expr")),
        INSERT_EXPRESSION => Ok(text_member(object, "directive")),
        BOOLEAN => match member(object, "bool") {
            None => Ok(ContextValue::Bool(false)),
            Some(Value::Bool(value)) => Ok(ContextValue::Bool(*value)),
            Some(_) => bail!("invalid TemplateToken boolean"),
        },
        NUMBER => number(member(object, "num")),
        NULL => Ok(ContextValue::Null),
        other => bail!("unknown TemplateToken type {other}"),
    }
}

fn text_member(object: &serde_json::Map<String, Value>, name: &str) -> ContextValue {
    member(object, name)
        .and_then(Value::as_str)
        .map_or(ContextValue::Null, |text| {
            ContextValue::String(text.to_owned())
        })
}

fn sequence(object: &serde_json::Map<String, Value>) -> Result<ContextValue> {
    let Some(items) = member(object, "seq") else {
        return Ok(ContextValue::Array(Vec::new()));
    };
    items
        .as_array()
        .context("TemplateToken sequence must be an array")?
        .iter()
        .map(template_token_context_value)
        .collect::<Result<Vec<_>>>()
        .map(ContextValue::Array)
}

fn mapping(object: &serde_json::Map<String, Value>) -> Result<ContextValue> {
    let pairs: &[Value] = match member(object, "map") {
        None => &[],
        Some(pairs) => pairs
            .as_array()
            .context("TemplateToken mapping must be an array")?,
    };
    let entries = pairs
        .iter()
        .map(|pair| {
            let pair = pair
                .as_object()
                .context("TemplateToken mapping pair must be an object")?;
            let key = member(pair, "key").context("TemplateToken mapping pair has no key")?;
            let key = match template_token_context_value(key)? {
                ContextValue::String(key) | ContextValue::BigInteger(key) => key,
                ContextValue::Bool(key) => key.to_string(),
                ContextValue::Number(key) => key.to_string(),
                _ => bail!("TemplateToken mapping key must be scalar"),
            };
            let value = member(pair, "value")
                .map_or(Ok(ContextValue::Null), template_token_context_value)?;
            Ok((key, value))
        })
        .collect::<Result<Vec<_>>>()?;
    ContextValue::object(entries).context("duplicate TemplateToken mapping key")
}

fn number(value: Option<&Value>) -> Result<ContextValue> {
    match value {
        None => Ok(ContextValue::Number(0.into())),
        Some(Value::Number(value)) => Ok(ContextValue::Number(value.clone())),
        Some(Value::String(text)) => match text.as_str() {
            "NaN" => Ok(ContextValue::non_finite(NonFinite::NaN)),
            "Infinity" => Ok(ContextValue::non_finite(NonFinite::PositiveInfinity)),
            "-Infinity" => Ok(ContextValue::non_finite(NonFinite::NegativeInfinity)),
            _ => bail!("invalid TemplateToken number"),
        },
        Some(_) => bail!("invalid TemplateToken number"),
    }
}
