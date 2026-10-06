use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_mise_core::MiseError;
use velnor_actions_mise_core::toml_scan::{TomlValue, parse_toml};

use super::invalid;

const TASK_FIELDS: &[&str] = &[
    "run",
    "description",
    "usage",
    "alias",
    "depends",
    "depends_post",
    "wait_for",
    "sources",
    "outputs",
    "hide",
    "quiet",
    "silent",
    "raw",
    "shell",
];

pub(super) fn validated_tasks(
    bytes: &str,
    selected: &str,
) -> Result<velnor_actions_mise_core::toml_scan::TomlDoc, MiseError> {
    let doc = parse_toml(bytes)
        .map_err(|error| invalid("mise_config", format!("{}:{}", error.line, error.problem)))?;
    let mut names = BTreeSet::new();
    let mut edges = BTreeMap::<String, Vec<String>>::new();
    for (path, _) in &doc.sections {
        if path.first().is_some_and(|part| part == "tasks") {
            if path.len() != 2 {
                return Err(invalid("mise_tasks", "nested_task_tables_unsupported"));
            }
            if !velnor_actions_contract::config::is_valid_mise_task_name(&path[1]) {
                return Err(invalid("mise_task_name", &path[1]));
            }
            names.insert(path[1].clone());
        }
    }
    let mut aliases = BTreeSet::new();
    for assignment in &doc.assignments {
        if let Some(refs) = validate_assignment(assignment)? {
            let task = &assignment.path[1];
            if assignment.path[2] == "alias" {
                for alias in refs {
                    if names.contains(&alias) || !aliases.insert(alias.clone()) {
                        return Err(invalid("mise_task_alias", "duplicate_or_shadowed_alias"));
                    }
                }
            } else {
                edges.entry(task.clone()).or_default().extend(refs);
            }
        }
    }
    names.extend(aliases);
    for refs in edges.values() {
        for name in refs {
            if !names.contains(name) {
                return Err(invalid("mise_task_dependency", format!("missing:{name}")));
            }
        }
    }
    if !names.contains(selected) {
        return Err(invalid("mise_task", format!("missing:{selected}")));
    }
    Ok(doc)
}

fn validate_assignment(
    assignment: &velnor_actions_mise_core::toml_scan::TomlAssignment,
) -> Result<Option<Vec<String>>, MiseError> {
    if assignment.path.iter().any(|part| {
        matches!(
            part.as_str(),
            "includes" | "include" | "task_config" | "task_dir" | "task_dirs"
        )
    }) {
        return Err(invalid(
            "mise_config",
            "includes_and_file_tasks_unsupported",
        ));
    }
    if assignment.path.first().is_none_or(|part| part != "tasks") {
        return Ok(None);
    }
    if assignment.path.len() != 3 || !TASK_FIELDS.contains(&assignment.path[2].as_str()) {
        return Err(invalid("mise_task_field", assignment.path.join(".")));
    }
    if contains_template(&assignment.value) {
        return Err(invalid("mise_task", "dynamic_templates_unsupported"));
    }
    if !["depends", "depends_post", "wait_for", "alias"].contains(&assignment.path[2].as_str()) {
        return Ok(None);
    }
    let refs = strings(&assignment.value)?;
    for name in &refs {
        if !velnor_actions_contract::config::is_valid_mise_task_name(name) {
            return Err(invalid("mise_task_reference", name));
        }
    }
    Ok(Some(refs))
}

pub(super) fn task_projection(
    doc: &velnor_actions_mise_core::toml_scan::TomlDoc,
    bytes: &str,
) -> String {
    let lines: Vec<&str> = bytes.split_inclusive('\n').collect();
    let mut projection = String::new();
    for (index, (path, start)) in doc.sections.iter().enumerate() {
        if path.first().is_some_and(|part| part == "tasks") {
            let end = doc
                .sections
                .get(index + 1)
                .map_or(lines.len(), |(_, line)| *line as usize - 1);
            projection.extend(lines[*start as usize - 1..end].iter().copied());
            if !projection.ends_with('\n') {
                projection.push('\n');
            }
        }
    }
    projection
}

fn strings(value: &TomlValue) -> Result<Vec<String>, MiseError> {
    match value {
        TomlValue::Str(value) => Ok(vec![value.clone()]),
        TomlValue::Array(values) => values
            .iter()
            .map(|value| match value {
                TomlValue::Str(value) => Ok(value.clone()),
                _ => Err(invalid("mise_task_reference", "expected_string")),
            })
            .collect(),
        _ => Err(invalid("mise_task_reference", "expected_string_or_array")),
    }
}

fn contains_template(value: &TomlValue) -> bool {
    match value {
        TomlValue::Str(value) => value.contains("{{") || value.contains("{%"),
        TomlValue::Array(values) => values.iter().any(contains_template),
        _ => false,
    }
}
