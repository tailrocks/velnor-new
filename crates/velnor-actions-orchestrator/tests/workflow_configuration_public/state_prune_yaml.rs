// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Map, Value};

type Fail = Box<dyn Error>;

struct Line {
    indent: usize,
    text: String,
}

pub(super) fn parse(source: &str) -> Result<Value, Fail> {
    let mut lines: Vec<Line> = source
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| Line {
            indent: line.len() - line.trim_start().len(),
            text: line
                .trim_start()
                .split_once(" # zizmor: ignore[")
                .map_or(line.trim_start(), |(head, _)| head)
                .to_owned(),
        })
        .collect();
    let mut at = 0;
    let value = block(&mut lines, &mut at, 0)?;
    if at != lines.len() {
        return Err(format!("unparsed yaml from line {at}: {}", lines[at].text).into());
    }
    Ok(value)
}

fn scalar(text: &str) -> Result<Value, Fail> {
    match text {
        "{}" => Ok(Value::Object(Map::new())),
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        _ if text.starts_with('"') => Ok(serde_json::from_str(text)?),
        _ if text.starts_with(['[', '{', '&', '*', '|', '>']) => {
            Err(format!("yaml form outside the prune workflow subset: {text}").into())
        }
        _ => Ok(Value::String(text.to_owned())),
    }
}

fn split_key(text: &str) -> Result<(String, &str), Fail> {
    let (key, rest) = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted
            .find("\":")
            .ok_or_else(|| format!("unterminated quoted key: {text}"))?
            + 1;
        (serde_json::from_str(&text[..=end])?, &text[end + 2..])
    } else {
        let end = text
            .find(':')
            .ok_or_else(|| format!("no mapping key in: {text}"))?;
        (text[..end].to_owned(), &text[end + 1..])
    };
    if !rest.is_empty() && !rest.starts_with(' ') {
        return Err(format!("malformed mapping entry: {text}").into());
    }
    Ok((key, rest.trim_start()))
}

fn block(lines: &mut [Line], at: &mut usize, indent: usize) -> Result<Value, Fail> {
    if lines
        .get(*at)
        .is_some_and(|line| line.text.starts_with("- "))
    {
        return list(lines, at, indent);
    }
    let mut map = Map::new();
    while let Some(line) = lines.get(*at) {
        if line.indent != indent || line.text.starts_with("- ") {
            break;
        }
        let (key, rest) = split_key(&line.text)?;
        *at += 1;
        let value = if rest.is_empty() {
            match lines.get(*at) {
                Some(next) if next.indent > indent => {
                    let child = next.indent;
                    block(lines, at, child)?
                }
                Some(next) if next.indent == indent && next.text.starts_with("- ") => {
                    list(lines, at, indent)?
                }
                _ => Value::Null,
            }
        } else {
            scalar(rest)?
        };
        if map.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate key {key}").into());
        }
    }
    Ok(Value::Object(map))
}

fn list(lines: &mut [Line], at: &mut usize, indent: usize) -> Result<Value, Fail> {
    let mut items = Vec::new();
    while let Some(line) = lines.get(*at) {
        if line.indent != indent || !line.text.starts_with("- ") {
            break;
        }
        let rest = line.text[2..].to_owned();
        if split_key(&rest).is_ok() {
            lines[*at] = Line {
                indent: indent + 2,
                text: rest,
            };
            items.push(block(lines, at, indent + 2)?);
        } else {
            *at += 1;
            items.push(scalar(&rest)?);
        }
    }
    Ok(Value::Array(items))
}

fn operand(token: &str, context: &Value) -> Result<Value, Fail> {
    if let Some(literal) = token
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return Ok(Value::String(literal.to_owned()));
    }
    if !token.starts_with("github.") {
        return Err(format!("unsupported operand {token}").into());
    }
    let mut node = context;
    for part in token.split('.') {
        node = node
            .get(part)
            .ok_or_else(|| format!("context {token} is absent from the payload at {part}"))?;
    }
    Ok(node.clone())
}

fn comparison(clause: &str, context: &Value) -> Result<bool, Fail> {
    let tokens: Vec<&str> = clause.split_whitespace().collect();
    let [left, op, right] = tokens.as_slice() else {
        return Err(format!("unsupported clause {clause}").into());
    };
    let equal = operand(left, context)? == operand(right, context)?;
    match *op {
        "==" => Ok(equal),
        "!=" => Ok(!equal),
        _ => Err(format!("unsupported operator {op}").into()),
    }
}

pub(super) fn evaluate(expression: &str, context: &Value) -> Result<bool, Fail> {
    for any in expression.split(" || ") {
        let mut all = true;
        for clause in any.split(" && ") {
            all &= comparison(clause, context)?;
        }
        if all {
            return Ok(true);
        }
    }
    Ok(false)
}
