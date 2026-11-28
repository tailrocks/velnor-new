// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/workflow_command.rs

use std::collections::BTreeMap;

use super::output::Output;
use super::{Annotation, AnnotationLevel};

const RESERVED_TOKENS: [&str; 16] = [
    "stop-commands",
    "pause-logging",
    "set-env",
    "set-output",
    "save-state",
    "add-mask",
    "add-path",
    "add-matcher",
    "remove-matcher",
    "debug",
    "warning",
    "error",
    "notice",
    "group",
    "endgroup",
    "echo",
];

const UNSUPPORTED_TOKEN: &str = "You cannot use a endToken that is an empty string, the string 'pause-logging', or another workflow command. For more information see: https://docs.github.com/actions/learn-github-actions/workflow-commands-for-github-actions#example-stopping-and-starting-workflow-commands or opt into insecure command execution by setting the `ACTIONS_ALLOW_UNSECURE_STOPCOMMAND_TOKENS` environment variable to `true`.";

struct WorkflowCommand<'a> {
    name: &'a str,
    properties: BTreeMap<String, String>,
    data: String,
}

fn simple_upper(character: char) -> char {
    match character {
        '\u{131}' | '\u{17f}' => character,
        '\u{1f80}'..='\u{1f87}' | '\u{1f90}'..='\u{1f97}' | '\u{1fa0}'..='\u{1fa7}' => {
            char::from_u32(u32::from(character) + 8).unwrap_or(character)
        }
        '\u{1fb3}' => '\u{1fbc}',
        '\u{1fc3}' => '\u{1fcc}',
        '\u{1ff3}' => '\u{1ffc}',
        _ => {
            let mut upper = character.to_uppercase();
            match (upper.next(), upper.next()) {
                (Some(single), None) => single,
                _ => character,
            }
        }
    }
}

fn fold(text: &str) -> String {
    text.chars().map(simple_upper).collect()
}

fn unescape_data(value: &str) -> String {
    value
        .replace("%0D", "\r")
        .replace("%0A", "\n")
        .replace("%25", "%")
}

fn unescape_property(value: &str) -> String {
    unescape_data(&value.replace("%3A", ":").replace("%2C", ","))
}

fn unescape_legacy(value: &str) -> String {
    value
        .replace("%3B", ";")
        .replace("%0D", "\r")
        .replace("%0A", "\n")
        .replace("%5D", "]")
        .replace("%25", "%")
}

fn parse_properties(
    properties: &str,
    separator: char,
    unescape: fn(&str) -> String,
) -> BTreeMap<String, String> {
    properties
        .split(separator)
        .filter_map(|property| {
            let (name, value) = property.split_once('=')?;
            Some((name.trim().to_ascii_lowercase(), unescape(value)))
        })
        .collect()
}

fn command_info(header: &str) -> (&str, &str) {
    header.split_once(' ').unwrap_or((header, ""))
}

fn parse_modern(line: &str) -> Option<WorkflowCommand<'_>> {
    let rest = line.trim_start().strip_prefix("::")?;
    let (header, data) = rest.split_once("::")?;
    let (name, properties) = command_info(header);
    Some(WorkflowCommand {
        name,
        properties: parse_properties(properties, ',', unescape_property),
        data: unescape_data(data),
    })
}

fn parse_legacy(line: &str) -> Option<WorkflowCommand<'_>> {
    let start = line.find("##[")?;
    let (header, data) = line[start + 3..].split_once(']')?;
    let (name, properties) = command_info(header);
    Some(WorkflowCommand {
        name,
        properties: parse_properties(properties, ';', unescape_legacy),
        data: unescape_legacy(data),
    })
}

fn registered(name: &str, resume: Option<&str>) -> bool {
    !name.eq_ignore_ascii_case("pause-logging")
        && (RESERVED_TOKENS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
            || resume.is_some_and(|token| token == fold(name)))
}

fn parse<'a>(line: &'a str, resume: Option<&str>) -> Option<WorkflowCommand<'a>> {
    parse_modern(line)
        .filter(|command| registered(command.name, resume))
        .or_else(|| parse_legacy(line).filter(|command| registered(command.name, resume)))
}

fn trim_integer(text: &str) -> &str {
    text.trim_matches(|c: char| matches!(c, '\u{9}'..='\u{d}' | ' '))
}

fn validate_range(properties: &mut BTreeMap<String, String>) {
    let text = |properties: &BTreeMap<String, String>, key: &str| properties.get(key).cloned();
    let parsed = |value: &Option<String>| {
        value
            .as_deref()
            .and_then(|v| trim_integer(v).parse::<i32>().ok())
    };
    let mut line = text(properties, "line");
    let end_line = text(properties, "endline");
    let mut column = text(properties, "col");
    let end_column = text(properties, "endcolumn");
    let (line_number, end_line_number) = (parsed(&line), parsed(&end_line));
    let (column_number, end_column_number) = (parsed(&column), parsed(&end_column));
    let mut has_line = line_number.is_some();
    let has_end_line = end_line_number.is_some();
    let mut has_column = column_number.is_some();
    let has_end_column = end_column_number.is_some();
    let any_column = has_column || has_end_column;
    if has_end_line && !has_line {
        properties.insert("line".to_owned(), end_line.clone().unwrap_or_default());
        has_line = true;
        line.clone_from(&end_line);
    }
    if has_end_column && !has_column {
        properties.insert("col".to_owned(), end_column.clone().unwrap_or_default());
        has_column = true;
        column.clone_from(&end_column);
    }
    let drop_columns = |properties: &mut BTreeMap<String, String>| {
        properties.remove("col");
        properties.remove("endcolumn");
    };
    if !has_line && any_column {
        drop_columns(properties);
    }
    if has_end_line && line != end_line && any_column {
        drop_columns(properties);
    }
    if has_line
        && has_end_line
        && end_line_number.unwrap_or_default() < line_number.unwrap_or_default()
    {
        properties.remove("line");
        properties.remove("endline");
    }
    if has_column
        && has_end_column
        && end_column_number.unwrap_or_default() < column_number.unwrap_or_default()
    {
        drop_columns(properties);
    }
}

fn number(properties: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    trim_integer(properties.get(key)?).parse().ok()
}

fn text(properties: &BTreeMap<String, String>, key: &str) -> Option<String> {
    properties
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
}

fn annotation(level: AnnotationLevel, mut command: WorkflowCommand<'_>) -> Annotation {
    validate_range(&mut command.properties);
    let properties = &command.properties;
    Annotation {
        level,
        title: text(properties, "title"),
        file: text(properties, "file"),
        line: number(properties, "line"),
        end_line: number(properties, "endline"),
        column: number(properties, "col"),
        end_column: number(properties, "endcolumn"),
        step: None,
        message: command.data,
    }
}

fn add_mask(data: &str, out: &mut Output<'_>) {
    if data.trim().is_empty() {
        out.annotate(
            AnnotationLevel::Warning,
            "Can't add secret mask for empty string in ##[add-mask] command.",
        );
    } else {
        out.masker.add(data);
    }
}

fn stop(token: &str, out: &mut Output<'_>) {
    let lowered = token.to_ascii_lowercase();
    if token.is_empty() || RESERVED_TOKENS.contains(&lowered.as_str()) {
        out.annotate(
            AnnotationLevel::Error,
            format!("Unable to process command '::stop-commands::{token}' successfully."),
        );
        out.annotate(AnnotationLevel::Error, UNSUPPORTED_TOKEN);
        out.issues.fail_command();
        return;
    }
    if token.encode_utf16().count() > 6 {
        out.masker.add(token);
    }
    out.issues.stop(fold(token));
}

pub(super) fn apply(line: &str, out: &mut Output<'_>) {
    let Some(command) = parse(line, out.issues.stop_token()) else {
        return;
    };
    let name = command.name.to_ascii_lowercase();
    if let Some(token) = out.issues.stop_token() {
        if fold(command.name) == token {
            out.issues.resume();
        }
        return;
    }
    match name.as_str() {
        "stop-commands" => stop(&command.data, out),
        "add-mask" => add_mask(&command.data, out),
        "error" => out.push(annotation(AnnotationLevel::Error, command)),
        "warning" => out.push(annotation(AnnotationLevel::Warning, command)),
        "notice" => out.push(annotation(AnnotationLevel::Notice, command)),
        _ => {}
    }
}

fn relative(path: &str, folder: &str) -> Option<String> {
    if !path.starts_with(folder) || path.len() == folder.len() {
        return None;
    }
    let rest = if folder.ends_with('/') {
        &path[folder.len()..]
    } else if path[folder.len()..].starts_with('/') {
        &path[folder.len() + 1..]
    } else {
        return None;
    };
    (!rest.is_empty()).then(|| rest.to_owned())
}

pub(super) fn relativize(annotation: &mut Annotation, workspace: &str) {
    if let Some(file) = annotation
        .file
        .as_deref()
        .and_then(|file| relative(file, workspace))
    {
        annotation.file = Some(file);
    }
}
