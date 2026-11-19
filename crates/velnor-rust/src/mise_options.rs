// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use toml::Value;

pub(crate) fn option_value(name: &str, key: &str, value: &Value, rust: bool) -> Option<String> {
    if name.starts_with("npm:") {
        if ![
            "npm_args",
            "pnpm_args",
            "bun_args",
            "aube_args",
            "allow_builds",
            "trust_policy_excludes",
            "allow_low_downloads",
            "allow_exotic_deps",
            "checksum",
        ]
        .contains(&key)
        {
            return None;
        }
        if ["allow_builds", "allow_exotic_deps", "trust_policy_excludes"].contains(&key) {
            return npm_list(key, value);
        }
    }
    if rust && ["components", "targets"].contains(&key) {
        let values: Option<Vec<&str>> = match value {
            Value::String(text) => Some(text.split(',').collect()),
            Value::Array(values) => values.iter().map(Value::as_str).collect(),
            _ => None,
        };
        if let Some(values) = values {
            return Some(
                values
                    .into_iter()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    Some(raw_value(value))
}

fn npm_list(key: &str, value: &Value) -> Option<String> {
    if key != "trust_policy_excludes" {
        let text = raw_value(value);
        if text.eq_ignore_ascii_case("false") {
            return None;
        }
        if text.eq_ignore_ascii_case("true") {
            return Some("true".to_owned());
        }
    }
    let values: Option<Vec<&str>> = match value {
        Value::String(text) => Some(vec![text]),
        Value::Array(values) => values.iter().map(Value::as_str).collect(),
        _ => None,
    };
    let Some(values) = values else {
        return Some(raw_value(value));
    };
    let values: Vec<_> = values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    (!values.is_empty()).then(|| format!("{values:?}"))
}

fn raw_value(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}
