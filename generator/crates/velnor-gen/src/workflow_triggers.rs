// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

fn names_push(on: &Value) -> bool {
    match on {
        Value::String(event) => event == "push",
        Value::Array(events) => events.iter().any(names_push),
        Value::Object(events) => events.contains_key("push"),
        _ => false,
    }
}

pub(crate) fn runs_on_push(text: &str) -> bool {
    serde_saphyr::from_str::<Value>(text)
        .ok()
        .and_then(|doc| doc.get("on").map(names_push))
        .unwrap_or(false)
}
