// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Whether a change to the Mise config or lock can reach one `mise-task` lint.
//!
//! A path-only rule would send every Mise edit to every lint. The lint really
//! reads its own task (and the tasks that task depends on), the pins of its own
//! tools, and the top-level settings that apply to every task. When the base and
//! head text of the file is known, anything else is an unrelated edit. When it is
//! not known or does not parse, the answer is "reached" (fail open).

use std::collections::BTreeSet;

use toml::{Table, Value};
use velnor_model::{ChangeSet, RepoPath};

use crate::mise::{MISE_CONFIGS, MISE_LOCK};

const TASK_LINKS: [&str; 3] = ["depends", "depends_post", "wait_for"];
const PER_TOOL_OR_TASK: [&str; 2] = ["tools", "tasks"];

#[must_use]
pub fn is_mise_file(path: &RepoPath) -> bool {
    *path == MISE_LOCK || MISE_CONFIGS.contains(path)
}

pub(crate) fn reaches_task(
    path: &RepoPath,
    task: &str,
    tools: &[String],
    changes: &ChangeSet,
) -> bool {
    let (Some(base), Some(head)) = (
        changes.base_files.get(path).and_then(|text| parse(text)),
        changes.head_files.get(path).and_then(|text| parse(text)),
    ) else {
        return true;
    };
    if *path == MISE_LOCK {
        return pins_differ(&base, &head, tools);
    }
    outside_differs(&base, &head)
        || pins_differ(&base, &head, tools)
        || tasks_differ(&base, &head, task)
}

fn parse(text: &str) -> Option<Table> {
    toml::from_str(text).ok()
}

fn outside_differs(base: &Table, head: &Table) -> bool {
    base.keys()
        .chain(head.keys())
        .filter(|key| !PER_TOOL_OR_TASK.contains(&key.as_str()))
        .any(|key| base.get(key) != head.get(key))
}

fn section(table: &Table, key: &str, wanted: &dyn Fn(&str) -> bool) -> Vec<(String, Value)> {
    table
        .get(key)
        .and_then(Value::as_table)
        .map_or_else(Vec::new, |section| {
            section
                .iter()
                .filter(|(name, _)| wanted(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
}

fn differs(base: &Table, head: &Table, key: &str, wanted: &dyn Fn(&str) -> bool) -> bool {
    section(base, key, wanted) != section(head, key, wanted)
}

fn tool_key(name: &str, tool: &str) -> bool {
    let bare = name.rsplit([':', '/']).next().unwrap_or(name);
    name == tool || bare == tool
}

fn pins_differ(base: &Table, head: &Table, tools: &[String]) -> bool {
    let wanted = |name: &str| tools.iter().any(|tool| tool_key(name, tool));
    differs(base, head, "tools", &wanted)
}

fn tasks_differ(base: &Table, head: &Table, task: &str) -> bool {
    let reached: BTreeSet<String> = task_closure(head, task)
        .union(&task_closure(base, task))
        .cloned()
        .collect();
    differs(base, head, "tasks", &|name| reached.contains(name))
}

fn task_closure(table: &Table, task: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::from([task.to_owned()]);
    let mut open = vec![task.to_owned()];
    while let Some(name) = open.pop() {
        let Some(definition) = table
            .get("tasks")
            .and_then(Value::as_table)
            .and_then(|tasks| tasks.get(&name))
        else {
            continue;
        };
        for link in TASK_LINKS {
            for dependency in links(definition.get(link)) {
                if seen.insert(dependency.clone()) {
                    open.push(dependency);
                }
            }
        }
    }
    seen
}

fn links(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(name)) => vec![name.clone()],
        Some(Value::Array(items)) => items.iter().flat_map(|item| links(Some(item))).collect(),
        Some(Value::Table(table)) => links(table.get("task")),
        _ => Vec::new(),
    }
}
