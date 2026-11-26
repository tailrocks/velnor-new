// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/workflow_command.rs

use std::collections::BTreeMap;

use super::mask::Masker;
use super::{Annotation, AnnotationLevel};

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

fn parse_properties(properties: &str) -> BTreeMap<String, String> {
    properties
        .split(',')
        .filter_map(|property| {
            let (name, value) = property.split_once('=')?;
            Some((name.trim().to_owned(), unescape_property(value)))
        })
        .collect()
}

fn parse(line: &str) -> Option<WorkflowCommand<'_>> {
    let rest = line.trim_start().strip_prefix("::")?;
    let (header, data) = rest.split_once("::")?;
    let (name, properties) = header
        .split_once(' ')
        .map_or((header, BTreeMap::new()), |(name, properties)| {
            (name, parse_properties(properties))
        });
    Some(WorkflowCommand {
        name,
        properties,
        data: unescape_data(data),
    })
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
        end_line: number(properties, "endLine"),
        column: number(properties, "col"),
        end_column: number(properties, "endColumn"),
        step: None,
        message: command.data,
    }
}

pub(super) fn apply(line: &str, masker: &mut Masker, annotations: &mut Vec<Annotation>) {
    let Some(command) = parse(line) else {
        return;
    };
    let level = match command.name.to_ascii_lowercase().as_str() {
        "add-mask" => {
            if command.data.trim().is_empty() {
                annotations.push(Annotation::new(
                    AnnotationLevel::Warning,
                    "Can't add secret mask for empty string in ##[add-mask] command.",
                ));
            } else {
                masker.add(&command.data);
            }
            return;
        }
        "error" => AnnotationLevel::Error,
        "warning" => AnnotationLevel::Warning,
        "notice" => AnnotationLevel::Notice,
        _ => return,
    };
    annotations.push(annotation(level, command));
}
