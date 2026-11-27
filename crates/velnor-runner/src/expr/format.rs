// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::env::ExprEnv;
use super::error::ExprError;
use super::eval::evaluate_node;
use super::memory::{MemoryCounter, Subject};
use super::node::Node;
use super::number::format_number;
use super::value::{Value, ValueKind};
use crate::context_value::{json_string_utf16_units, write_json_string};
use std::ops::Range;

pub(crate) fn evaluate_format(
    node: &Node,
    args: &[Node],
    env: &ExprEnv,
) -> Result<Value, ExprError> {
    let [format, rest @ ..] = args else {
        return Err(ExprError::evaluation(
            "Invalid number of function arguments",
        ));
    };
    let format = evaluate_node(format, env)?.convert_to_string();
    let mut memory = MemoryCounter::new(Subject::Node(node));
    format_template(&format, rest.len(), &mut memory, |index| {
        match rest.get(index) {
            Some(arg) => evaluate_node(arg, env),
            None => Ok(Value::Null),
        }
    })
    .map(Value::String)
}

struct Placeholder {
    arg_index: usize,
    specifiers: Range<usize>,
    rbrace: usize,
}

fn byte_at(bytes: &[u8], position: usize) -> u8 {
    bytes.get(position).copied().unwrap_or(0)
}

fn find_byte(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|&byte| byte == needle)
        .map(|position| position + from)
}

#[derive(Clone, Copy)]
enum Scan {
    Unknown,
    Absent,
    At(usize),
}

struct NextByte {
    needle: u8,
    scan: Scan,
}

impl NextByte {
    fn new(needle: u8) -> Self {
        Self {
            needle,
            scan: Scan::Unknown,
        }
    }

    fn from(&mut self, bytes: &[u8], index: usize) -> Option<usize> {
        match self.scan {
            Scan::Absent => return None,
            Scan::At(position) if position >= index => return Some(position),
            Scan::At(_) | Scan::Unknown => {}
        }
        let found = find_byte(bytes, index, self.needle);
        self.scan = found.map_or(Scan::Absent, Scan::At);
        found
    }
}

fn read_placeholder(bytes: &[u8], lbrace: usize, rbrace: Option<usize>) -> Option<Placeholder> {
    if rbrace.is_none_or(|rbrace| rbrace <= lbrace + 1) {
        return None;
    }
    let (arg_index, end_arg_index) = read_arg_index(bytes, lbrace + 1)?;
    let (specifiers, rbrace) = read_format_specifiers(bytes, end_arg_index + 1)?;
    Some(Placeholder {
        arg_index,
        specifiers,
        rbrace,
    })
}

enum Segment {
    Text(Range<usize>),
    Argument {
        index: usize,
        specifiers: Range<usize>,
    },
}

fn visit_text(
    visit: &mut impl FnMut(Segment) -> Result<(), ExprError>,
    range: Range<usize>,
) -> Result<(), ExprError> {
    if range.is_empty() {
        Ok(())
    } else {
        visit(Segment::Text(range))
    }
}

fn scan_template(
    format: &str,
    argument_count: usize,
    mut visit: impl FnMut(Segment) -> Result<(), ExprError>,
) -> Result<(), ExprError> {
    let bytes = format.as_bytes();
    let mut index = 0usize;
    let invalid = || ExprError::evaluation(format!("Invalid format string: {format}"));
    let mut opening = NextByte::new(b'{');
    let mut closing = NextByte::new(b'}');

    while index < bytes.len() {
        let lbrace = opening.from(bytes, index);
        let rbrace = closing.from(bytes, index);

        match (lbrace, rbrace) {
            (Some(lbrace), rbrace) if rbrace.is_none_or(|rbrace| rbrace > lbrace) => {
                if byte_at(bytes, lbrace + 1) == b'{' {
                    visit_text(&mut visit, index..lbrace + 1)?;
                    index = lbrace + 2;
                    continue;
                }
                let placeholder = read_placeholder(bytes, lbrace, rbrace).ok_or_else(invalid)?;
                if placeholder.arg_index >= argument_count {
                    return Err(ExprError::evaluation(format!(
                        "The following format string references more arguments than were supplied: {format}"
                    )));
                }
                visit_text(&mut visit, index..lbrace)?;
                visit(Segment::Argument {
                    index: placeholder.arg_index,
                    specifiers: placeholder.specifiers,
                })?;
                index = placeholder.rbrace + 1;
            }
            (_, Some(rbrace)) => {
                if byte_at(bytes, rbrace + 1) != b'}' {
                    return Err(invalid());
                }
                visit_text(&mut visit, index..rbrace + 1)?;
                index = rbrace + 2;
            }
            _ => {
                visit_text(&mut visit, index..bytes.len())?;
                break;
            }
        }
    }

    Ok(())
}

fn format_template(
    format: &str,
    argument_count: usize,
    memory: &mut MemoryCounter<'_>,
    mut resolve: impl FnMut(usize) -> Result<Value, ExprError>,
) -> Result<String, ExprError> {
    scan_template(format, argument_count, |segment| match segment {
        Segment::Text(range) => memory.add(&format[range]),
        Segment::Argument { .. } => Ok(()),
    })?;
    let mut cache: Vec<Option<(ValueKind, String)>> = vec![None; argument_count];
    let mut result = String::new();

    scan_template(format, argument_count, |segment| {
        match segment {
            Segment::Text(range) => result.push_str(&format[range]),
            Segment::Argument { index, specifiers } => {
                let slot = &mut cache[index];
                if slot.is_none() {
                    let value = resolve(index)?;
                    *slot = Some((value.kind(), value.convert_to_string()));
                }
                let Some((kind, argument)) = slot.as_ref() else {
                    return Ok(());
                };
                if !specifiers.is_empty() {
                    return Err(ExprError::evaluation(format!(
                        "The format specifiers '{}' are not valid for objects of type '{}'",
                        format[specifiers].replace("}}", "}"),
                        kind.as_str()
                    )));
                }
                memory.add(argument)?;
                result.push_str(argument);
            }
        }
        Ok(())
    })?;

    Ok(result)
}

fn read_arg_index(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let length = bytes
        .get(start..)?
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if length == 0 {
        return None;
    }
    let digits = std::str::from_utf8(&bytes[start..start + length]).ok()?;
    let parsed = digits.parse::<u8>().ok()?;
    Some((usize::from(parsed), start + length - 1))
}

fn read_format_specifiers(bytes: &[u8], start: usize) -> Option<(Range<usize>, usize)> {
    match byte_at(bytes, start) {
        b'}' => return Some((start..start, start)),
        b':' => {}
        _ => return None,
    }
    let mut index = start + 1;
    loop {
        let byte = *bytes.get(index)?;
        if byte != b'}' {
            index += 1;
        } else if bytes.get(index + 1).copied() == Some(b'}') {
            index += 2;
        } else {
            return Some((start + 1..index, index));
        }
    }
}

pub(crate) fn to_json(node: &Node, value: &Value) -> Result<String, ExprError> {
    let mut writer = JsonWriter {
        out: String::new(),
        memory: MemoryCounter::new(Subject::Node(node)),
    };
    write_json(value, &mut writer, 0, JsonPrefix::Root)?;
    Ok(writer.out)
}

struct JsonWriter<'a> {
    out: String,
    memory: MemoryCounter<'a>,
}

impl JsonWriter<'_> {
    fn push(&mut self, piece: &str) -> Result<(), ExprError> {
        self.memory.add(piece)?;
        self.out.push_str(piece);
        Ok(())
    }

    fn push_string(&mut self, prefix: &str, value: &str) -> Result<(), ExprError> {
        let units = prefix.encode_utf16().count() + json_string_utf16_units(value);
        self.memory.add_units(units)?;
        self.out.push_str(prefix);
        write_json_string(value, &mut self.out);
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
    writer: &mut JsonWriter<'_>,
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
        Value::String(value) => writer.push_string(&prefix, value),
        Value::Null => writer.push(&format!("{prefix}null")),
        Value::Boolean(value) => writer.push(&format!("{prefix}{value}")),
        Value::Number(value) => writer.push(&format!("{prefix}{}", format_number(*value))),
        Value::Array(_) => writer.push(&format!("{prefix}[]")),
        Value::Object(_) => writer.push(&format!("{prefix}{{}}")),
    }
}
