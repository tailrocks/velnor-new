use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use super::context::{FreshnessContext, py_repr};

const CHECK: &str = "lock-staleness";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

#[derive(Debug)]
pub(super) struct Declaration {
    pub(super) subject: String,
    pub(super) real: String,
    pub(super) requirement: TomlValue,
}

pub(super) fn check(
    ctx: &mut FreshnessContext,
    locked: &[JsonValue],
    declared: &[Declaration],
    members: &BTreeSet<String>,
) {
    if !super::lock_validation::validate(ctx, locked) {
        return;
    }
    let by_name = index_lock(locked);
    check_declared(ctx, declared, &by_name, members);
    check_membership(ctx, locked, &by_name, members);
}

fn index_lock(locked: &[JsonValue]) -> BTreeMap<String, Vec<JsonValue>> {
    let mut by_name: BTreeMap<String, Vec<JsonValue>> = BTreeMap::new();
    for entry in locked {
        by_name
            .entry(lock_field(entry, "name").to_owned())
            .or_default()
            .push(entry.clone());
    }
    by_name
}

fn check_declared(
    ctx: &mut FreshnessContext,
    declared: &[Declaration],
    by_name: &BTreeMap<String, Vec<JsonValue>>,
    members: &BTreeSet<String>,
) {
    let (mut passed, mut failed) = (0, 0);
    for dependency in declared {
        if check_requirement(ctx, dependency, by_name, members) {
            passed += 1;
        } else {
            failed += 1;
        }
    }
    ctx.info_row(
        CHECK,
        "(declared-summary)",
        &format!(
            "{passed} match, {failed} fail, {} locked names retained",
            by_name.len()
        ),
    );
}

fn check_requirement(
    ctx: &mut FreshnessContext,
    dependency: &Declaration,
    by_name: &BTreeMap<String, Vec<JsonValue>>,
    members: &BTreeSet<String>,
) -> bool {
    let Some(requirement) = dependency
        .requirement
        .as_str()
        .filter(|value| value.starts_with('='))
    else {
        ctx.fail_row(
            CHECK,
            &dependency.subject,
            &format!(
                "requirement {} is not exact `=x.y.z` (VER-2.26)",
                toml_repr(&dependency.requirement)
            ),
        );
        return false;
    };
    let want = requirement[1..].split('+').next().unwrap_or("");
    let matches = by_name
        .get(&dependency.real)
        .into_iter()
        .flatten()
        .filter(|entry| lock_field(entry, "version").split('+').next().unwrap_or("") == want)
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        let versions = by_name
            .get(&dependency.real)
            .into_iter()
            .flatten()
            .map(|entry| lock_field(entry, "version").to_owned())
            .collect::<BTreeSet<_>>();
        let rendered = py_repr(Some(&serde_json::json!(
            versions.into_iter().collect::<Vec<_>>()
        )));
        ctx.fail_row(
            CHECK,
            &dependency.subject,
            &format!(
                "declared {} has no locked identity (locked versions: {rendered})",
                toml_repr(&dependency.requirement)
            ),
        );
        return false;
    }
    check_resolved_source(ctx, dependency, want, requirement, &matches, members)
}

fn check_resolved_source(
    ctx: &mut FreshnessContext,
    dependency: &Declaration,
    want: &str,
    requirement: &str,
    matches: &[JsonValue],
    members: &BTreeSet<String>,
) -> bool {
    let sources = matches
        .iter()
        .map(|entry| lock_source(entry).to_owned())
        .collect::<BTreeSet<_>>();
    if sources.len() > 1 {
        let rendered = py_repr(Some(&serde_json::json!(sources)));
        ctx.fail_row(
            CHECK,
            &dependency.subject,
            &format!(
                "ambiguous identity: {} {want} resolves from {rendered}",
                dependency.real
            ),
        );
        return false;
    }
    let source = sources.iter().next().map_or("local", String::as_str);
    if members.contains(&dependency.real) && source != "local" {
        ctx.fail_row(
            CHECK,
            &dependency.subject,
            &format!("workspace member {} locked from {source}", dependency.real),
        );
        return false;
    }
    if members.contains(&dependency.real) {
        ctx.pass_row(
            CHECK,
            &dependency.subject,
            &format!("{requirement} @ workspace"),
        );
        return true;
    }
    if source != CRATES_IO {
        ctx.fail_row(
            CHECK,
            &dependency.subject,
            &format!("locked from non-registry source {source}"),
        );
        return false;
    }
    ctx.pass_row(
        CHECK,
        &dependency.subject,
        &format!("{requirement} @ registry"),
    );
    true
}

fn check_membership(
    ctx: &mut FreshnessContext,
    locked: &[JsonValue],
    by_name: &BTreeMap<String, Vec<JsonValue>>,
    members: &BTreeSet<String>,
) {
    let local_names = locked
        .iter()
        .filter(|entry| !json_truthy(entry.get("source")))
        .map(|entry| lock_field(entry, "name").to_owned())
        .collect::<BTreeSet<_>>();
    if &local_names == members {
        ctx.pass_row(
            CHECK,
            "(lock-membership)",
            &format!("{} members", members.len()),
        );
    } else {
        ctx.fail_row(
            CHECK,
            "(lock-membership)",
            &format!(
                "local lock {} != members {}",
                py_repr(Some(&serde_json::json!(local_names))),
                py_repr(Some(&serde_json::json!(members)))
            ),
        );
    }
    let roots = local_names.intersection(members).cloned().collect();
    check_lock_graph(ctx, locked, by_name, &roots);
}

fn check_lock_graph(
    ctx: &mut FreshnessContext,
    locked: &[JsonValue],
    by_name: &BTreeMap<String, Vec<JsonValue>>,
    roots: &BTreeSet<String>,
) {
    let mut queue = roots
        .iter()
        .flat_map(|name| by_name.get(name).into_iter().flatten().cloned())
        .collect::<Vec<_>>();
    let reachable = walk_lock_graph(ctx, &mut queue, by_name);
    let mut stranded = locked
        .iter()
        .filter(|entry| !reachable.contains(&lock_identity(entry)))
        .collect::<Vec<_>>();
    stranded.sort_by_key(|entry| lock_field(entry, "name"));
    if stranded.is_empty() {
        ctx.pass_row(
            CHECK,
            "(lock-graph)",
            &format!("{} locked packages reachable", locked.len()),
        );
    } else {
        for entry in stranded {
            ctx.fail_row(
                CHECK,
                "(lock-graph)",
                &format!(
                    "unreachable locked package {} {}",
                    lock_field(entry, "name"),
                    lock_field(entry, "version")
                ),
            );
        }
    }
}

fn walk_lock_graph(
    ctx: &mut FreshnessContext,
    queue: &mut Vec<JsonValue>,
    by_name: &BTreeMap<String, Vec<JsonValue>>,
) -> BTreeSet<(String, String, String)> {
    let mut reachable = queue.iter().map(lock_identity).collect::<BTreeSet<_>>();
    while let Some(entry) = queue.pop() {
        if let Some(edges) = entry.get("dependencies").and_then(JsonValue::as_array) {
            for value in edges {
                let Some(edge) = value.as_str() else {
                    continue;
                };
                let candidates = edge_candidates(edge, by_name);
                if candidates.is_empty() {
                    ctx.fail_row(
                        CHECK,
                        "(lock-graph)",
                        &format!("dangling edge {} -> {edge}", lock_field(&entry, "name")),
                    );
                }
                for candidate in candidates {
                    if reachable.insert(lock_identity(&candidate)) {
                        queue.push(candidate);
                    }
                }
            }
        }
    }
    reachable
}

fn edge_candidates(edge: &str, by_name: &BTreeMap<String, Vec<JsonValue>>) -> Vec<JsonValue> {
    let mut parts = edge.split(' ');
    let name = parts.next().unwrap_or("");
    let mut candidates = by_name.get(name).cloned().unwrap_or_default();
    if let Some(version) = parts.next() {
        candidates.retain(|entry| lock_field(entry, "version") == version);
    }
    if let Some(source) = parts.next() {
        let source = source.trim_matches(['(', ')']);
        candidates.retain(|entry| {
            entry
                .get("source")
                .and_then(JsonValue::as_str)
                .unwrap_or("")
                == source
        });
    }
    candidates
}

fn lock_identity(entry: &JsonValue) -> (String, String, String) {
    let version = lock_field(entry, "version")
        .split('+')
        .next()
        .unwrap_or("")
        .to_owned();
    (
        lock_field(entry, "name").to_owned(),
        version,
        lock_source(entry).to_owned(),
    )
}

fn lock_field<'a>(entry: &'a JsonValue, field: &str) -> &'a str {
    entry.get(field).and_then(JsonValue::as_str).unwrap_or("")
}

fn lock_source(entry: &JsonValue) -> &str {
    entry
        .get("source")
        .filter(|value| json_truthy(Some(value)))
        .and_then(JsonValue::as_str)
        .unwrap_or("local")
}

fn json_truthy(value: Option<&JsonValue>) -> bool {
    match value {
        None | Some(JsonValue::Null | JsonValue::Bool(false)) => false,
        Some(JsonValue::String(value)) => !value.is_empty(),
        Some(JsonValue::Number(value)) => value.as_f64().is_some_and(|number| number != 0.0),
        Some(JsonValue::Array(value)) => !value.is_empty(),
        Some(JsonValue::Object(value)) => !value.is_empty(),
        Some(JsonValue::Bool(true)) => true,
    }
}

fn toml_repr(value: &TomlValue) -> String {
    serde_json::to_value(value).map_or_else(|_| value.to_string(), |value| py_repr(Some(&value)))
}

#[cfg(test)]
#[path = "dependency_graph_tests.rs"]
mod tests;
