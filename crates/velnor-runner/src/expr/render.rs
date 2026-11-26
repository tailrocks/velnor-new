// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::node::{BinaryOperator, Node};
use super::number::format_number;
use super::template::{CLOSE, OPEN, find_close};
use super::value::Value;
use std::fmt::Write as _;

impl Node {
    pub(crate) fn to_expression(&self) -> String {
        match self {
            Node::Literal(value) => literal(value),
            Node::Wildcard => "*".to_owned(),
            Node::NamedValue(_, name) => name.clone(),
            Node::Index(target, index) => match index.as_ref() {
                Node::Literal(Value::String(key)) if is_legal_keyword(key) => {
                    format!("{}.{key}", target.to_expression())
                }
                other => format!("{}[{}]", target.to_expression(), other.to_expression()),
            },
            Node::Not(inner) => format!("!{}", inner.to_expression()),
            Node::And(nodes) => format!("({})", joined(nodes, " && ")),
            Node::Or(nodes) => format!("({})", joined(nodes, " || ")),
            Node::Binary(operator, left, right) => format!(
                "({} {} {})",
                left.to_expression(),
                operator_text(*operator),
                right.to_expression()
            ),
            Node::Function { name, args, .. } => format!("{name}({})", joined(args, ", ")),
        }
    }
}

fn joined(nodes: &[Node], separator: &str) -> String {
    nodes
        .iter()
        .map(Node::to_expression)
        .collect::<Vec<_>>()
        .join(separator)
}

fn operator_text(operator: BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Equal => "==",
        BinaryOperator::NotEqual => "!=",
        BinaryOperator::GreaterThan => ">",
        BinaryOperator::GreaterThanOrEqual => ">=",
        BinaryOperator::LessThan => "<",
        BinaryOperator::LessThanOrEqual => "<=",
    }
}

fn literal(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => format_number(*value),
        Value::String(value) => quoted(value),
        Value::Array(_) | Value::Object(_) => value.kind().as_str().to_owned(),
    }
}

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn is_legal_keyword(key: &str) -> bool {
    let mut characters = key.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn template_expression(source: &str) -> String {
    let mut format = String::new();
    let mut arguments = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find(OPEN) {
        push_text(&mut format, &rest[..start]);
        let expression = &rest[start + OPEN.len()..];
        let Some(end) = find_close(expression) else {
            push_text(&mut format, &rest[start..]);
            rest = "";
            break;
        };
        let _ = write!(format, "{{{}}}", arguments.len());
        arguments.push(expression[..end].trim());
        rest = &expression[end + CLOSE.len()..];
    }
    push_text(&mut format, rest);
    if matches!(arguments.as_slice(), [_]) && format == "{0}" {
        return arguments[0].to_owned();
    }
    let mut call = format!("format({}", quoted(&format));
    for argument in arguments {
        call.push_str(", ");
        call.push_str(argument);
    }
    call.push(')');
    call
}

fn push_text(format: &mut String, text: &str) {
    format.push_str(&text.replace('{', "{{").replace('}', "}}"));
}
