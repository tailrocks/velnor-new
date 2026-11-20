// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{
    AlreadyExists, Entry, Event, Scope, Timestamp, access_for, lookup, reserve,
};

pub(crate) fn rows(table: &str) -> Vec<Vec<&str>> {
    let lines = table.lines().map(str::trim).filter(|l| !l.is_empty());
    lines
        .map(|l| l.split(';').map(str::trim).collect())
        .collect()
}

pub(crate) fn number(text: &str) -> u64 {
    let parsed = text.parse::<u64>();
    assert!(parsed.is_ok(), "not a number: {text:?}");
    parsed.unwrap_or_default()
}

pub(crate) fn list(field: &str) -> Vec<&str> {
    field.split_whitespace().filter(|w| *w != "-").collect()
}

pub(crate) fn scope(spec: &str) -> Scope {
    match (spec.strip_prefix('#'), spec.strip_prefix("t:")) {
        (Some(text), _) => Scope::pull_merge(number(text)),
        (_, Some(name)) => Scope::tag(name),
        _ => Scope::branch(spec),
    }
}

pub(crate) fn short(scope: &Scope) -> String {
    let path = scope.0.replace("refs/heads/", "").replace("/merge", "");
    path.replace("refs/tags/", "t:").replace("refs/pull/", "#")
}

pub(crate) fn event(spec: &str) -> Event {
    let part: Vec<&str> = spec.splitn(3, ':').collect();
    let name = part[1].to_owned();
    match part[0] {
        "push" => Event::Push { branch: name },
        "tag" => Event::Tag { name },
        "queue" => Event::MergeGroup { queue_branch: name },
        _ => Event::PullRequest {
            number: number(&name),
            base_branch: part[2].to_owned(),
        },
    }
}

pub(crate) fn entry(spec: &str) -> Entry {
    let part: Vec<&str> = spec.split('|').collect();
    let number = |at: usize, or: u64| part.get(at).map_or(or, |n| number(n));
    let created = number(3, 0);
    Entry {
        scope: scope(part[0]),
        key: part[1].to_owned(),
        version: if part[2].is_empty() { "v1" } else { part[2] }.to_owned(),
        size_bytes: number(4, 1),
        created: Timestamp(created),
        last_access: Timestamp(number(5, created)),
    }
}

pub(crate) fn entries(field: &str) -> Vec<Entry> {
    list(field).into_iter().map(entry).collect()
}

pub(crate) fn found(
    saved: &[Entry],
    event_spec: &str,
    key: &str,
    restore: &str,
    version: &str,
) -> String {
    let restore_keys: Vec<String> = list(restore).into_iter().map(str::to_owned).collect();
    let access = access_for(&event(event_spec), "main");
    lookup(saved, &access, key, &restore_keys, version).map_or_else(
        || "-".to_owned(),
        |hit| format!("{}|{}", short(&hit.scope), hit.key),
    )
}

pub(crate) fn reserved(saved: &[Entry], event_spec: &str, key: &str, version: &str) -> String {
    let access = access_for(&event(event_spec), "main");
    reserve(saved, &access, key, version).map_or_else(
        |AlreadyExists(id)| format!("{}|{}|{}", short(&id.scope), id.key, id.version),
        |()| "-".to_owned(),
    )
}

pub(crate) fn saved(saved: &mut Vec<Entry>, event_spec: &str, key: &str) -> String {
    let outcome = reserved(saved, event_spec, key, "v1");
    if outcome == "-" {
        let write = access_for(&event(event_spec), "main").write;
        saved.push(entry(&format!("{}|{key}||{}", short(&write), saved.len())));
    }
    outcome
}
