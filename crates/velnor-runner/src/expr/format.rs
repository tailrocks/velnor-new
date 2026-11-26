// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::env::ExprEnv;
use super::error::ExprError;
use super::eval::evaluate_node;
use super::node::Node;
use super::number::format_number;
use super::value::Value;

pub(crate) fn evaluate_format(args: &[Node], env: &ExprEnv) -> Result<Value, ExprError> {
    let [format, rest @ ..] = args else {
        return Err(ExprError::evaluation(
            "Invalid number of function arguments",
        ));
    };
    let format = evaluate_node(format, env)?.convert_to_string();
    format_template(&format, rest.len(), |index| match rest.get(index) {
        Some(arg) => evaluate_node(arg, env),
        None => Ok(Value::Null),
    })
    .map(Value::String)
}

struct Placeholder {
    arg_index: usize,
    specifiers: String,
    rbrace: usize,
}

fn char_at(chars: &[char], position: usize) -> char {
    chars.get(position).copied().unwrap_or('\0')
}

fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
    chars
        .get(from..)?
        .iter()
        .position(|&c| c == needle)
        .map(|position| position + from)
}

fn read_placeholder(chars: &[char], lbrace: usize, rbrace: Option<usize>) -> Option<Placeholder> {
    if rbrace.is_none_or(|rbrace| rbrace <= lbrace + 1) {
        return None;
    }
    let (arg_index, end_arg_index) = read_arg_index(chars, lbrace + 1)?;
    let (specifiers, rbrace) = read_format_specifiers(chars, end_arg_index + 1)?;
    Some(Placeholder {
        arg_index,
        specifiers,
        rbrace,
    })
}

pub(crate) fn format_template(
    format: &str,
    argument_count: usize,
    mut resolve: impl FnMut(usize) -> Result<Value, ExprError>,
) -> Result<String, ExprError> {
    let chars: Vec<char> = format.chars().collect();
    let mut cache: Vec<Option<String>> = vec![None; argument_count];
    let mut result = String::new();
    let mut index = 0usize;
    let invalid = || ExprError::evaluation(format!("Invalid format string: {format}"));

    while index < chars.len() {
        let lbrace = find_char(&chars, index, '{');
        let rbrace = find_char(&chars, index, '}');

        match (lbrace, rbrace) {
            (Some(lbrace), rbrace) if rbrace.is_none_or(|rbrace| rbrace > lbrace) => {
                if char_at(&chars, lbrace + 1) == '{' {
                    result.extend(&chars[index..=lbrace]);
                    index = lbrace + 2;
                    continue;
                }
                let placeholder = read_placeholder(&chars, lbrace, rbrace).ok_or_else(invalid)?;
                if placeholder.arg_index >= argument_count {
                    return Err(ExprError::evaluation(format!(
                        "The following format string references more arguments than were supplied: {format}"
                    )));
                }
                if !placeholder.specifiers.is_empty() {
                    return Err(ExprError::evaluation(format!(
                        "The format specifiers '{}' are not valid for objects of type '{}'",
                        placeholder.specifiers,
                        resolve(placeholder.arg_index)?.kind().as_str()
                    )));
                }
                result.extend(&chars[index..lbrace]);
                let slot = &mut cache[placeholder.arg_index];
                if slot.is_none() {
                    *slot = Some(resolve(placeholder.arg_index)?.convert_to_string());
                }
                result.push_str(slot.as_deref().unwrap_or_default());
                index = placeholder.rbrace + 1;
            }
            (_, Some(rbrace)) => {
                if char_at(&chars, rbrace + 1) != '}' {
                    return Err(invalid());
                }
                result.extend(&chars[index..=rbrace]);
                index = rbrace + 2;
            }
            _ => {
                result.extend(&chars[index..]);
                break;
            }
        }
    }

    Ok(result)
}

fn read_arg_index(chars: &[char], start: usize) -> Option<(usize, usize)> {
    let length = chars
        .get(start..)?
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if length == 0 {
        return None;
    }
    let digits: String = chars[start..start + length].iter().collect();
    let parsed = digits.parse::<u8>().ok()?;
    Some((usize::from(parsed), start + length - 1))
}

fn read_format_specifiers(chars: &[char], start: usize) -> Option<(String, usize)> {
    match char_at(chars, start) {
        '}' => return Some((String::new(), start)),
        ':' => {}
        _ => return None,
    }
    let mut specifiers = String::new();
    let mut index = start + 1;
    loop {
        let c = *chars.get(index)?;
        if c != '}' {
            specifiers.push(c);
            index += 1;
        } else if chars.get(index + 1).copied() == Some('}') {
            specifiers.push('}');
            index += 2;
        } else {
            return Some((specifiers, index));
        }
    }
}

pub(crate) fn to_json(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out, 0, JsonPrefix::Root);
    out
}

#[derive(Clone, Copy)]
enum JsonPrefix {
    Root,
    MappingValue,
    Element(bool),
}

fn write_json(value: &Value, out: &mut String, level: usize, prefix: JsonPrefix) {
    let indent = "  ".repeat(level);
    let prefix = match prefix {
        JsonPrefix::Root => String::new(),
        JsonPrefix::MappingValue => ": ".to_string(),
        JsonPrefix::Element(first) => {
            format!("{}\n{indent}", if first { "" } else { "," })
        }
    };

    match value {
        Value::Object(object) if !object.entries().is_empty() => {
            out.push_str(&prefix);
            out.push('{');
            for (position, (key, entry)) in object.entries().iter().enumerate() {
                let key = Value::String(key.clone());
                write_json(&key, out, level + 1, JsonPrefix::Element(position == 0));
                write_json(entry, out, level + 1, JsonPrefix::MappingValue);
            }
            out.push('\n');
            out.push_str(&indent);
            out.push('}');
        }
        Value::Array(array) if !array.items().is_empty() => {
            out.push_str(&prefix);
            out.push('[');
            for (position, item) in array.items().iter().enumerate() {
                write_json(item, out, level + 1, JsonPrefix::Element(position == 0));
            }
            out.push('\n');
            out.push_str(&indent);
            out.push(']');
        }
        other => {
            out.push_str(&prefix);
            out.push_str(&match other {
                Value::Null => "null".to_string(),
                Value::Boolean(value) => value.to_string(),
                Value::Number(value) => format_number(*value),
                Value::String(value) => serde_json::Value::String(value.clone()).to_string(),
                Value::Array(_) => "[]".to_string(),
                Value::Object(_) => "{}".to_string(),
            });
        }
    }
}
