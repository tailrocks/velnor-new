// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use anyhow::{Context, Result, bail};
use serde_json::{Map, Number, Value};

use crate::context_value::{ContextValue, NonFinite};

const STRING: i64 = 0;
const ARRAY: i64 = 1;
const DICTIONARY: i64 = 2;
const BOOLEAN: i64 = 3;
const NUMBER: i64 = 4;
const CASE_SENSITIVE_DICTIONARY: i64 = 5;

pub(super) fn member<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    member_or_null(object, name).filter(|value| !value.is_null())
}

fn member_or_null<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object.get(name).or_else(|| {
        object
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    })
}

pub fn pipeline_context_value(value: &Value) -> Result<ContextValue> {
    let object = match value {
        Value::String(text) => return Ok(ContextValue::String(text.clone())),
        Value::Bool(flag) => return Ok(ContextValue::Bool(*flag)),
        Value::Number(number) => return Ok(ContextValue::Number(number.clone())),
        Value::Null | Value::Array(_) => return Ok(ContextValue::Null),
        Value::Object(object) => object,
    };
    let kind = match member_or_null(object, "t") {
        None => STRING,
        Some(Value::Number(number)) if !number.is_f64() => number
            .as_i64()
            .context("PipelineContextData type discriminator out of range")?,
        Some(_) => return Ok(ContextValue::Null),
    };
    match kind {
        STRING => Ok(ContextValue::String(
            member(object, "s")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        )),
        ARRAY => member(object, "a")
            .map(|items| {
                items
                    .as_array()
                    .context("PipelineContextData array must be an array")?
                    .iter()
                    .map(pipeline_context_value)
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()
            .map(|items| ContextValue::Array(items.unwrap_or_default())),
        DICTIONARY | CASE_SENSITIVE_DICTIONARY => {
            dictionary(object, kind == CASE_SENSITIVE_DICTIONARY)
        }
        BOOLEAN => match member(object, "b") {
            None => Ok(ContextValue::Bool(false)),
            Some(Value::Bool(value)) => Ok(ContextValue::Bool(*value)),
            Some(_) => bail!("invalid PipelineContextData boolean"),
        },
        NUMBER => number(member(object, "n")),
        other => bail!("unknown PipelineContextData type {other}"),
    }
}

fn dictionary(object: &Map<String, Value>, case_sensitive: bool) -> Result<ContextValue> {
    let pairs = member(object, "d")
        .map(|pairs| {
            pairs
                .as_array()
                .context("PipelineContextData dictionary must be an array")
        })
        .transpose()?
        .map_or(&[][..], Vec::as_slice);
    let entries = pairs
        .iter()
        .map(|pair| {
            let pair = pair
                .as_object()
                .context("PipelineContextData dictionary pair must be an object")?;
            let key = member(pair, "k")
                .and_then(Value::as_str)
                .context("PipelineContextData dictionary pair has no string key")?;
            let value = member(pair, "v").map_or(Ok(ContextValue::Null), pipeline_context_value)?;
            Ok((key.to_owned(), value))
        })
        .collect::<Result<Vec<_>>>()?;
    let built = if case_sensitive {
        ContextValue::case_sensitive_object(entries)
    } else {
        ContextValue::object(entries)
    };
    built.context("invalid PipelineContextData dictionary")
}

fn number(value: Option<&Value>) -> Result<ContextValue> {
    match value {
        None => Ok(ContextValue::Number(Number::from(0))),
        Some(Value::Number(value)) => Ok(ContextValue::Number(value.clone())),
        Some(Value::String(text)) => match text.as_str() {
            "NaN" => Ok(ContextValue::non_finite(NonFinite::NaN)),
            "Infinity" => Ok(ContextValue::non_finite(NonFinite::PositiveInfinity)),
            "-Infinity" => Ok(ContextValue::non_finite(NonFinite::NegativeInfinity)),
            _ => bail!("invalid PipelineContextData number"),
        },
        Some(_) => bail!("invalid PipelineContextData number"),
    }
}
