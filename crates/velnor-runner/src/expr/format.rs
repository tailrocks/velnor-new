// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::env::ExprEnv;
use super::error::ExprError;
use super::eval::evaluate_node;
use super::node::Node;
use super::number::format_number;
use super::value::{Value, ValueKind};
use crate::context_value::write_json_string;

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

const MAX_MEMORY_BYTES: usize = 10 * 1024 * 1024;
const STRING_BASE_OVERHEAD: usize = 26;
const BYTES_PER_UTF16_UNIT: usize = 2;

pub(crate) struct MemoryCounter {
    function: &'static str,
    bytes: usize,
}

impl MemoryCounter {
    pub(crate) fn new(function: &'static str) -> Self {
        Self { function, bytes: 0 }
    }

    pub(crate) fn add(&mut self, value: &str) -> Result<(), ExprError> {
        if value.is_empty() {
            return Ok(());
        }
        let units = value.encode_utf16().count();
        self.add_bytes(
            STRING_BASE_OVERHEAD.saturating_add(units.saturating_mul(BYTES_PER_UTF16_UNIT)),
        )
    }

    pub(crate) fn add_bytes(&mut self, bytes: usize) -> Result<(), ExprError> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_MEMORY_BYTES {
            return Err(ExprError::evaluation(format!(
                "The maximum allowed memory size was exceeded while evaluating the following expression: {}",
                self.function
            )));
        }
        Ok(())
    }
}

enum Segment {
    Text(String),
    Argument { index: usize, specifiers: String },
}

fn parse_template(format: &str, argument_count: usize) -> Result<Vec<Segment>, ExprError> {
    let chars: Vec<char> = format.chars().collect();
    let mut segments = Vec::new();
    let mut index = 0usize;
    let invalid = || ExprError::evaluation(format!("Invalid format string: {format}"));

    while index < chars.len() {
        let lbrace = find_char(&chars, index, '{');
        let rbrace = find_char(&chars, index, '}');

        match (lbrace, rbrace) {
            (Some(lbrace), rbrace) if rbrace.is_none_or(|rbrace| rbrace > lbrace) => {
                if char_at(&chars, lbrace + 1) == '{' {
                    segments.push(Segment::Text(chars[index..=lbrace].iter().collect()));
                    index = lbrace + 2;
                    continue;
                }
                let placeholder = read_placeholder(&chars, lbrace, rbrace).ok_or_else(invalid)?;
                if placeholder.arg_index >= argument_count {
                    return Err(ExprError::evaluation(format!(
                        "The following format string references more arguments than were supplied: {format}"
                    )));
                }
                segments.push(Segment::Text(chars[index..lbrace].iter().collect()));
                segments.push(Segment::Argument {
                    index: placeholder.arg_index,
                    specifiers: placeholder.specifiers,
                });
                index = placeholder.rbrace + 1;
            }
            (_, Some(rbrace)) => {
                if char_at(&chars, rbrace + 1) != '}' {
                    return Err(invalid());
                }
                segments.push(Segment::Text(chars[index..=rbrace].iter().collect()));
                index = rbrace + 2;
            }
            _ => {
                segments.push(Segment::Text(chars[index..].iter().collect()));
                break;
            }
        }
    }

    Ok(segments)
}

pub(crate) fn format_template(
    format: &str,
    argument_count: usize,
    mut resolve: impl FnMut(usize) -> Result<Value, ExprError>,
) -> Result<String, ExprError> {
    let mut memory = MemoryCounter::new("format");
    let mut cache: Vec<Option<(ValueKind, String)>> = vec![None; argument_count];
    let mut result = String::new();

    for segment in parse_template(format, argument_count)? {
        match segment {
            Segment::Text(text) => {
                memory.add(&text)?;
                result.push_str(&text);
            }
            Segment::Argument { index, specifiers } => {
                let slot = &mut cache[index];
                if slot.is_none() {
                    let value = resolve(index)?;
                    *slot = Some((value.kind(), value.convert_to_string()));
                }
                let Some((kind, argument)) = slot.as_ref() else {
                    continue;
                };
                if !specifiers.is_empty() {
                    return Err(ExprError::evaluation(format!(
                        "The format specifiers '{specifiers}' are not valid for objects of type '{}'",
                        kind.as_str()
                    )));
                }
                memory.add(argument)?;
                result.push_str(argument);
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

pub(crate) fn to_json(value: &Value) -> Result<String, ExprError> {
    let mut writer = JsonWriter {
        out: String::new(),
        memory: MemoryCounter::new("toJSON"),
    };
    write_json(value, &mut writer, 0, JsonPrefix::Root)?;
    Ok(writer.out)
}

struct JsonWriter {
    out: String,
    memory: MemoryCounter,
}

impl JsonWriter {
    fn push(&mut self, piece: &str) -> Result<(), ExprError> {
        self.memory.add(piece)?;
        self.out.push_str(piece);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum JsonPrefix {
    Root,
    MappingValue,
    Element(bool),
}

fn write_json(
    value: &Value,
    writer: &mut JsonWriter,
    level: usize,
    prefix: JsonPrefix,
) -> Result<(), ExprError> {
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
            writer.push(&format!("{prefix}{{"))?;
            for (position, (key, entry)) in object.entries().iter().enumerate() {
                let key = Value::String(key.clone());
                write_json(&key, writer, level + 1, JsonPrefix::Element(position == 0))?;
                write_json(entry, writer, level + 1, JsonPrefix::MappingValue)?;
            }
            writer.push(&format!("\n{indent}}}"))
        }
        Value::Array(array) if !array.items().is_empty() => {
            writer.push(&format!("{prefix}["))?;
            for (position, item) in array.items().iter().enumerate() {
                write_json(item, writer, level + 1, JsonPrefix::Element(position == 0))?;
            }
            writer.push(&format!("\n{indent}]"))
        }
        other => {
            let text = match other {
                Value::Null => "null".to_string(),
                Value::Boolean(value) => value.to_string(),
                Value::Number(value) => format_number(*value),
                Value::String(value) => {
                    let mut quoted = String::new();
                    write_json_string(value, &mut quoted);
                    quoted
                }
                Value::Array(_) => "[]".to_string(),
                Value::Object(_) => "{}".to_string(),
            };
            writer.push(&format!("{prefix}{text}"))
        }
    }
}
