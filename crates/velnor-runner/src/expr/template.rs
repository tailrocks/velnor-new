// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use super::env::ExprEnv;
use super::error::{ExprError, ParseError, ParseErrorKind};
use super::eval::evaluate_node;
use super::memory::{MemoryCounter, Subject};
use super::node::Node;
use super::parser::{MAX_LENGTH, parse};
use super::render::{Argument, template_expression};
use super::value::Value;

const OPEN: &str = "${{";
const CLOSE: &str = "}}";
const FORMAT_MAX_PARAMETERS: usize = 255;

pub(crate) enum Part<'a> {
    Text(&'a str),
    Expression { source: &'a str, node: Node },
}

fn find_close(source: &str) -> Option<usize> {
    let mut in_string = false;
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => in_string = !in_string,
            b'}' if !in_string && bytes.get(index + 1) == Some(&b'}') => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

fn split<'a>(source: &'a str, env: &ExprEnv) -> Result<Vec<Part<'a>>, ExprError> {
    let mut parts = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find(OPEN) {
        if start > 0 {
            parts.push(Part::Text(&rest[..start]));
        }
        let expression = &rest[start + OPEN.len()..];
        let Some(end) = find_close(expression) else {
            return Err(ExprError::evaluation(format!(
                "The expression is not closed. An unescaped {OPEN} sequence was found, but the closing {CLOSE} sequence was not found."
            )));
        };
        let source = expression[..end].trim();
        let Some(node) = parse(source, env)? else {
            return Err(ExprError::evaluation("An expression was expected"));
        };
        parts.push(Part::Expression { source, node });
        rest = &expression[end + CLOSE.len()..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest));
    }
    Ok(parts)
}

pub fn evaluate_template(source: &str, env: &ExprEnv) -> Result<String, ExprError> {
    let parts = split(source, env)?;
    if parts.iter().all(|part| matches!(part, Part::Text(_))) {
        return Ok(source.to_owned());
    }
    let expression = template_expression(&parts, Argument::Source);
    if expression.encode_utf16().count() > MAX_LENGTH {
        return Err(ParseError::without_token(ParseErrorKind::ExceededMaxLength).into());
    }
    let spans = parts
        .iter()
        .filter(|part| matches!(part, Part::Expression { .. }))
        .count();
    if spans >= FORMAT_MAX_PARAMETERS {
        parse(&expression, env)?;
    }
    let single_span = spans == 1 && parts.len() == 1;
    let mut rendered = String::with_capacity(source.len());
    let mut memory = MemoryCounter::new(Subject::Template(&parts));
    for part in &parts {
        match part {
            Part::Text(text) => {
                memory.add(text)?;
                rendered.push_str(text);
            }
            Part::Expression { node, .. } => {
                let value = evaluate_node(node, env)?;
                if single_span && matches!(value, Value::Array(_) | Value::Object(_)) {
                    return Err(ExprError::evaluation("Expected a string"));
                }
                let value = value.convert_to_string();
                memory.add(&value)?;
                rendered.push_str(&value);
            }
        }
    }
    Ok(rendered)
}
