// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use toml::Value;
use velnor_model::Weakening;

use crate::weaken::Make;

const KINDS: &[&str] = &["test", "bench", "bin", "example"];
const DISCOVERY: &[(&str, &str)] = &[
    ("autolib", "src/lib.rs"),
    ("autoexamples", "examples/"),
    ("autobenches", "benches/"),
    ("autobins", "src/bin/"),
];

fn is_false(table: Option<&Value>, key: &str) -> bool {
    table
        .and_then(|table| table.get(key))
        .and_then(Value::as_bool)
        == Some(false)
}

fn default_features(parsed: &toml::Table) -> BTreeSet<String> {
    let features = parsed.get("features").and_then(Value::as_table);
    let mut enabled = BTreeSet::new();
    let mut pending = vec!["default".to_owned()];
    while let Some(name) = pending.pop() {
        if !enabled.insert(name.clone()) {
            continue;
        }
        let implied = features
            .and_then(|features| features.get(&name))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|item| !item.contains('/') && !item.starts_with("dep:"));
        pending.extend(implied.map(str::to_owned));
    }
    enabled
}

pub(crate) fn targets(parsed: &toml::Table, owner: &str, make: &Make<'_>) -> Vec<Weakening> {
    let mut found = Vec::new();
    let package = parsed.get("package");
    for &(key, folder) in DISCOVERY {
        if is_false(package, key) {
            found.push(make(
                format!("{key} = false"),
                format!("stops {owner} from finding the files in {folder} as targets"),
            ));
        }
    }
    let lib = parsed.get("lib");
    if is_false(lib, "doc") {
        found.push(make(
            "[lib] doc = false".to_owned(),
            format!("leaves the library of {owner} out of cargo doc"),
        ));
    }
    if is_false(lib, "harness") {
        found.push(make(
            "[lib] harness = false".to_owned(),
            format!("stops the library of {owner} from collecting its #[test] functions"),
        ));
    }
    let enabled = default_features(parsed);
    for kind in KINDS.iter().copied() {
        let entries = parsed
            .get(kind)
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for (index, target) in entries.enumerate() {
            let name = target
                .get("name")
                .or_else(|| target.get("path"))
                .and_then(Value::as_str)
                .map_or_else(|| format!("unnamed #{index}"), ToOwned::to_owned);
            let head = format!("[[{kind}]] name = \"{name}\"");
            if kind == "bin" && is_false(Some(target), "doc") {
                found.push(make(
                    format!("{head} doc = false"),
                    format!("leaves the {kind} target {name} of {owner} out of cargo doc"),
                ));
            }
            if is_false(Some(target), "harness") {
                found.push(make(
                    format!("{head} harness = false"),
                    format!("stops the {kind} target {name} of {owner} from collecting its #[test] functions"),
                ));
            }
            let required: Vec<&str> = target
                .get("required-features")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|feature| !enabled.contains(*feature))
                .collect();
            if !required.is_empty() {
                found.push(make(
                    format!("{head} required-features = {required:?}"),
                    format!("builds the {kind} target {name} of {owner} only when {} is enabled, and the generated run enables no extra feature", required.join(", ")),
                ));
            }
        }
    }
    found
}
