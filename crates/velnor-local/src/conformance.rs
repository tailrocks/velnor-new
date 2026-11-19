// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

/// The values a run's placeholders stand for, such as `ref` for `${ref}`.
pub type Values = BTreeMap<String, String>;

const TYPE_WORDS: [&str; 8] = [
    "string", "number", "boolean", "object", "array", "null", "sha", "absent",
];

/// Paths compared by value however the stored file writes them: the identity claims that name
/// the repository and the refs the run was triggered on, and the cache and token scope lists.
const EXACT: [&str; 5] = [
    "claims.repository",
    "claims.ref",
    "claims.defaultRef",
    "cache_scopes",
    "scp",
];

/// The placeholder values of the run that wrote one event, read from its payload.
#[must_use]
pub fn run_values(payload: &Value) -> Values {
    let repository = &payload["repository"];
    let mut values = Values::new();
    if let Some(name) = repository["full_name"].as_str() {
        values.insert("repository".to_owned(), name.to_owned());
    }
    if let Some(branch) = repository["default_branch"].as_str() {
        values.insert("defaultRef".to_owned(), format!("refs/heads/{branch}"));
    }
    let git_ref = match payload.get("pull_request") {
        Some(pull) if pull["merged"].as_bool() == Some(true) => pull["base"]["ref"]
            .as_str()
            .map(|base| format!("refs/heads/{base}")),
        Some(pull) => pull["number"]
            .as_u64()
            .map(|number| format!("refs/pull/{number}/merge")),
        None => payload["ref"].as_str().map(str::to_owned),
    };
    if let Some(git_ref) = git_ref {
        values.insert("ref".to_owned(), git_ref);
    }
    values
}

#[must_use]
pub fn observations(log: &str) -> Vec<Value> {
    let mut found = prefixed(log);
    found.extend(plain(log));
    found
}

fn prefixed(log: &str) -> Vec<Value> {
    let lines: Vec<Option<&str>> = log.lines().map(|line| line.strip_prefix("| ")).collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if lines[at] == Some("{") {
            let end = (at..lines.len())
                .find(|&index| lines[index] == Some("}"))
                .unwrap_or(lines.len() - 1);
            let text: Vec<&str> = lines[at..=end].iter().flatten().copied().collect();
            found.extend(observation(&text.join("\n")));
            at = end;
        }
        at += 1;
    }
    found
}

fn plain(log: &str) -> Vec<Value> {
    let lines: Vec<&str> = log.lines().collect();
    let mut found = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if !line.starts_with('{') {
            continue;
        }
        let block = if *line == "{" {
            lines[at..]
                .iter()
                .position(|line| *line == "}")
                .map_or(&lines[at..at], |end| &lines[at..=at + end])
        } else {
            &lines[at..=at]
        };
        found.extend(observation(&block.join("\n")));
    }
    found
}

fn observation(text: &str) -> Option<Value> {
    let document = serde_json::from_str::<Value>(text).ok()?;
    (document["probe"].is_string() && document["legs"].is_object()).then_some(document)
}

#[must_use]
pub fn differences(stored: &Value, observed: &Value, values: &Values) -> Vec<String> {
    let mut found = Vec::new();
    walk(stored, observed, "", values, &mut found);
    found
}

fn walk(stored: &Value, observed: &Value, path: &str, values: &Values, found: &mut Vec<String>) {
    if EXACT.contains(&path) || is_placeholder(stored) {
        let expected = substitute(stored, values);
        if &expected != observed {
            found.push(format!("{path}: GitHub {expected}, local {observed}"));
        }
    } else if let (Value::Object(stored), Value::Object(observed)) = (stored, observed) {
        let keys: BTreeSet<&String> = stored.keys().chain(observed.keys()).collect();
        for key in keys {
            let at = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            match (stored.get(key), observed.get(key)) {
                (Some(stored), Some(observed)) => walk(stored, observed, &at, values, found),
                (Some(stored), None) => found.push(format!("{at}: GitHub only {stored}")),
                (None, Some(observed)) => found.push(format!("{at}: local only {observed}")),
                (None, None) => {}
            }
        }
    } else if let Value::String(name) = stored
        && TYPE_WORDS.contains(&name.as_str())
    {
        let kind = kind_of(observed);
        if kind != *name {
            found.push(format!(
                "{path}: GitHub {}, local {}",
                json!(name),
                json!(kind)
            ));
        }
    } else if stored != observed {
        found.push(format!("{path}: GitHub {stored}, local {observed}"));
    }
}

fn is_placeholder(value: &Value) -> bool {
    value.as_str().is_some_and(|text| text.contains("${"))
}

/// A volatile field is observed as its type, whether the probe printed the type or the value.
fn kind_of(observed: &Value) -> String {
    match observed {
        Value::String(text) if TYPE_WORDS.contains(&text.as_str()) => text.clone(),
        Value::String(_) => "string".to_owned(),
        Value::Number(_) => "number".to_owned(),
        Value::Bool(_) => "boolean".to_owned(),
        Value::Null => "null".to_owned(),
        Value::Array(_) => "array".to_owned(),
        Value::Object(_) => "object".to_owned(),
    }
}

fn substitute(value: &Value, values: &Values) -> Value {
    match value {
        Value::String(text) => {
            Value::String(values.iter().fold(text.clone(), |text, (name, value)| {
                text.replace(&format!("${{{name}}}"), value)
            }))
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| substitute(item, values)).collect())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), substitute(item, values)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn report(
    logs: &[(String, String)],
    stored: &dyn Fn(&str) -> Option<Value>,
    values: &dyn Fn(&str) -> Values,
) -> (Vec<String>, bool) {
    let mut seen: BTreeMap<(String, String), (&str, Value)> = BTreeMap::new();
    for (name, log) in logs {
        for document in observations(log) {
            let probe = document["probe"].as_str().unwrap_or_default().to_owned();
            for (leg, value) in document["legs"].as_object().into_iter().flatten() {
                seen.insert((probe.clone(), leg.clone()), (name, value.clone()));
            }
        }
    }
    let probes: BTreeSet<&String> = seen.keys().map(|(probe, _)| probe).collect();
    let (mut lines, mut reproduced) = (Vec::new(), true);
    for probe in probes {
        let recorded = stored(probe)
            .map(|file| file["legs"].clone())
            .unwrap_or_default();
        let names: BTreeSet<String> = recorded
            .as_object()
            .into_iter()
            .flatten()
            .map(|(leg, _)| leg.clone())
            .chain(
                seen.keys()
                    .filter(|(name, _)| name == probe)
                    .map(|(_, leg)| leg.clone()),
            )
            .collect();
        for leg in names {
            let line = match (recorded.get(&leg), seen.get(&(probe.clone(), leg.clone()))) {
                (Some(want), Some((log, got))) => {
                    let found = differences(want, got, &values(log));
                    if found.is_empty() {
                        format!("{probe} / {leg} ({log}): reproduced")
                    } else {
                        let listed: Vec<String> =
                            found.iter().map(|line| format!("\n    {line}")).collect();
                        format!(
                            "{probe} / {leg} ({log}): {} differences{}",
                            found.len(),
                            listed.concat()
                        )
                    }
                }
                (Some(_), None) => format!("{probe} / {leg}: not observed"),
                (None, _) => format!("{probe} / {leg}: not stored in conformance/{probe}.json"),
            };
            reproduced &= line.ends_with(": reproduced");
            lines.push(line);
        }
    }
    let reproduced = reproduced && !lines.is_empty();
    (lines, reproduced)
}
