// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

pub(crate) struct WorkflowFacts {
    pub(crate) name: String,
    pub(crate) pushes: bool,
}

fn names_push(on: &Value) -> bool {
    match on {
        Value::String(event) => event == "push",
        Value::Array(events) => events.iter().any(names_push),
        Value::Object(events) => events.contains_key("push"),
        _ => false,
    }
}

fn declared_name(doc: &Value) -> Option<String> {
    match doc.get("name")? {
        Value::String(name) if !name.is_empty() => Some(name.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub(crate) fn workflow_facts(path: &str, text: &str) -> WorkflowFacts {
    match serde_saphyr::from_str::<Value>(text) {
        Ok(doc) => WorkflowFacts {
            name: declared_name(&doc).unwrap_or_else(|| path.to_owned()),
            pushes: doc.get("on").is_some_and(names_push),
        },
        Err(_) => WorkflowFacts {
            name: path.to_owned(),
            pushes: true,
        },
    }
}
