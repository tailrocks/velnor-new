// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::JobAuthorityLimit;

use super::inventory::Context;

const EVENTS: [&str; 3] = ["pull_request", "push", "merge_group"];
const _: () = assert!(EVENTS.len() <= JobAuthorityLimit::Contexts.max());
const UNCOVERED: [&str; 2] = ["schedule", "workflow_dispatch"];

static NO_CONFIG: Value = Value::Null;

fn declared<'a>(on: &'a Value, event: &str) -> Option<&'a Value> {
    match on {
        Value::String(name) => (name == event).then_some(&NO_CONFIG),
        Value::Array(names) => names
            .iter()
            .any(|name| name.as_str() == Some(event))
            .then_some(&NO_CONFIG),
        Value::Object(events) => events.get(event),
        _ => None,
    }
}

fn literal_list(config: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(list) = config.get(key) else {
        return Ok(Vec::new());
    };
    let names = list
        .as_array()
        .ok_or_else(|| format!("{key} must be a list"))?;
    let mut branches = Vec::with_capacity(names.len());
    for name in names {
        let name = name
            .as_str()
            .filter(|name| !name.is_empty() && !name.contains(['*', '?', '[', ']', '!']))
            .ok_or_else(|| format!("{key} must list literal branch names, found {name}"))?;
        branches.push(name.to_owned());
    }
    branches.sort();
    branches.dedup();
    Ok(branches)
}

pub(super) fn uncovered(on: &Value) -> Vec<&'static str> {
    UNCOVERED
        .into_iter()
        .filter(|event| declared(on, event).is_some())
        .collect()
}

pub(super) fn contexts(on: &Value, default_branch: &str) -> Result<Vec<Context>, String> {
    let mut contexts = Vec::new();
    for event in EVENTS {
        let Some(config) = declared(on, event) else {
            continue;
        };
        let default = vec![default_branch.to_owned()];
        let (branch_filters, base_branches) = match event {
            "pull_request" => (Vec::new(), literal_list(config, "branches")?),
            "push" => {
                if config.get("tags").is_some() {
                    return Err("push declares tags, which the inventory does not cover".to_owned());
                }
                let branches = literal_list(config, "branches")?;
                (
                    if branches.is_empty() {
                        default
                    } else {
                        branches
                    },
                    Vec::new(),
                )
            }
            _ => (default, Vec::new()),
        };
        contexts.push(Context::new(event, branch_filters, base_branches));
    }
    if contexts.is_empty() {
        return Err(format!(
            "the workflow triggers none of {}",
            EVENTS.join(", ")
        ));
    }
    Ok(contexts)
}
