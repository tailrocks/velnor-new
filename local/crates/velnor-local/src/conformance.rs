// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

#[must_use]
pub fn observations(log: &str) -> Vec<Value> {
    let lines: Vec<Option<&str>> = log.lines().map(|line| line.strip_prefix("| ")).collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if lines[at] == Some("{") {
            let end = (at..lines.len())
                .find(|&index| lines[index] == Some("}"))
                .unwrap_or(lines.len() - 1);
            let text: Vec<&str> = lines[at..=end].iter().flatten().copied().collect();
            if let Ok(document) = serde_json::from_str::<Value>(&text.join("\n"))
                && document["probe"].is_string()
                && document["legs"].is_object()
            {
                found.push(document);
            }
            at = end;
        }
        at += 1;
    }
    found
}

#[must_use]
pub fn differences(stored: &Value, observed: &Value) -> Vec<String> {
    let mut found = Vec::new();
    walk(stored, observed, "", &mut found);
    found
}

fn walk(stored: &Value, observed: &Value, path: &str, found: &mut Vec<String>) {
    if let (Value::Object(stored), Value::Object(observed)) = (stored, observed) {
        let keys: BTreeSet<&String> = stored.keys().chain(observed.keys()).collect();
        for key in keys {
            let at = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            match (stored.get(key), observed.get(key)) {
                (Some(stored), Some(observed)) => walk(stored, observed, &at, found),
                (Some(stored), None) => found.push(format!("{at}: GitHub only {stored}")),
                (None, Some(observed)) => found.push(format!("{at}: local only {observed}")),
                (None, None) => {}
            }
        }
    } else if stored != observed {
        found.push(format!("{path}: GitHub {stored}, local {observed}"));
    }
}

pub fn report(
    logs: &[(String, String)],
    stored: &dyn Fn(&str) -> Option<Value>,
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
                    let found = differences(want, got);
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
