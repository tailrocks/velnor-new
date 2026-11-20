// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use toml::Value;
use velnor_model::{RepoView, ToolPins};

use crate::error::Reason;
use crate::lint_error::LintFault;
use crate::lint_tools::{Lookup, lookup};
use crate::toml_file::table;

pub(crate) const TASK_FOLDERS: &[&str] = &[
    "mise-tasks/",
    ".mise-tasks/",
    "mise/tasks/",
    ".mise/tasks/",
    ".config/mise/tasks/",
];

#[derive(Default)]
struct Declared {
    tools: Vec<String>,
    depends: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Catalog {
    declared: BTreeMap<String, Declared>,
    files: BTreeSet<String>,
}

fn words(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(text)) => text
            .split_whitespace()
            .next()
            .map(str::to_owned)
            .into_iter()
            .collect(),
        Some(Value::Array(items)) => items.iter().flat_map(|item| words(Some(item))).collect(),
        Some(Value::Table(entry)) => words(entry.get("task")),
        _ => Vec::new(),
    }
}

fn declared(value: &Value) -> Declared {
    let Some(entry) = value.as_table() else {
        return Declared::default();
    };
    Declared {
        tools: entry
            .get("tools")
            .and_then(Value::as_table)
            .map(|tools| tools.keys().cloned().collect())
            .unwrap_or_default(),
        depends: ["depends", "depends_post"]
            .iter()
            .flat_map(|key| words(entry.get(*key)))
            .collect(),
    }
}

fn file_names(view: &RepoView) -> BTreeSet<String> {
    view.tracked_paths
        .iter()
        .filter_map(|path| {
            TASK_FOLDERS
                .iter()
                .find_map(|folder| path.as_str().strip_prefix(folder))
        })
        .flat_map(|rest| {
            let name = rest.replace('/', ":");
            let bare = rest
                .rsplit_once('.')
                .filter(|(stem, _)| !stem.is_empty() && !stem.ends_with('/'))
                .map(|(stem, _)| stem.replace('/', ":"));
            std::iter::once(name).chain(bare)
        })
        .collect()
}

pub(crate) fn catalog(view: &RepoView, tools: &ToolPins) -> Result<Catalog, Reason> {
    let mut declared_tasks = BTreeMap::new();
    for config in &tools.configs {
        let Some(bytes) = view.files.get(config) else {
            continue;
        };
        if let Some(Value::Table(tasks)) = table(bytes)?.get("tasks") {
            declared_tasks.extend(
                tasks
                    .iter()
                    .map(|(name, value)| (name.clone(), declared(value))),
            );
        }
    }
    Ok(Catalog {
        declared: declared_tasks,
        files: file_names(view),
    })
}

fn reached<'a>(catalog: &'a Catalog, start: &str) -> Vec<&'a str> {
    let mut seen: Vec<&str> = Vec::new();
    let mut queue = vec![start];
    while let Some(name) = queue.pop() {
        let Some((key, task)) = catalog.declared.get_key_value(name) else {
            continue;
        };
        if seen.contains(&key.as_str()) {
            continue;
        }
        seen.push(key);
        for dependency in &task.depends {
            match dependency.strip_suffix('*') {
                Some(prefix) => queue.extend(
                    catalog
                        .declared
                        .keys()
                        .map(String::as_str)
                        .filter(|name| name.starts_with(prefix)),
                ),
                None => queue.push(dependency),
            }
        }
    }
    seen
}

pub(crate) fn check(
    catalog: &Catalog,
    tools: &ToolPins,
    task: &str,
    named: &[String],
) -> Result<(), LintFault> {
    if !catalog.declared.contains_key(task) && !catalog.files.contains(task) {
        return Err(LintFault::TaskMissing(task.to_owned()));
    }
    let allowed: BTreeSet<&str> = named
        .iter()
        .filter_map(|name| match lookup(tools, name) {
            Lookup::One(key) => Some(key),
            _ => None,
        })
        .collect();
    for name in reached(catalog, task) {
        for used in &catalog.declared[name].tools {
            let key = match lookup(tools, used) {
                Lookup::One(key) => key,
                _ => used.as_str(),
            };
            if !allowed.contains(key) {
                return Err(LintFault::TaskToolUndeclared {
                    task: task.to_owned(),
                    tool: used.clone(),
                });
            }
        }
    }
    Ok(())
}
