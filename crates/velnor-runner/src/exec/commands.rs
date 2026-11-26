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

fn parse(line: &str) -> Option<WorkflowCommand<'_>> {
    parse_modern(line).or_else(|| parse_legacy(line))
}

fn number(properties: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    properties.get(key)?.parse().ok()
}

fn text(properties: &BTreeMap<String, String>, key: &str) -> Option<String> {
    properties
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
}

fn annotation(level: AnnotationLevel, command: WorkflowCommand<'_>) -> Annotation {
    let properties = &command.properties;
    Annotation {
        level,
        title: text(properties, "title"),
        file: text(properties, "file"),
        line: number(properties, "line"),
        end_line: number(properties, "endline"),
        column: number(properties, "col"),
        end_column: number(properties, "endcolumn"),
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
    if token.chars().count() > 6 {
        out.masker.add(token);
    }
    out.issues.stop(lowered);
}

pub(super) fn apply(line: &str, out: &mut Output<'_>) {
    let Some(command) = parse(line) else {
        return;
    };
    let name = command.name.to_ascii_lowercase();
    if let Some(token) = out.issues.stop_token() {
        if name == token {
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
