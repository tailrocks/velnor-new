//! Cargo lock graph identity and dependency checks.

mod graph;

mod identity;

use std::collections::BTreeMap;

use std::path::{Path, PathBuf};

use toml::Value;

use crate::context::FreshnessContext;

use super::{manifest_label, path_text, read_toml_row, workspace};

pub(super) fn check_workspace(
    ctx: &mut FreshnessContext,
    relative: &str,
    document: &Value,
) -> Vec<Value> {
    let label = manifest_label(relative);
    let inherited = document
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|table| table.get("dependencies"));
    if inherited.is_some_and(|value| !value.is_table()) {
        ctx.fail_row(
            "lock-staleness",
            &label,
            "workspace.dependencies must be a table",
        );
        return Vec::new();
    }
    let Some(locked) = read_lock(ctx, relative) else {
        return Vec::new();
    };
    let manifests = workspace::member_manifests(ctx, relative, document);
    if manifests.is_empty() {
        ctx.fail_row(
            "lock-staleness",
            &label,
            "no workspace member manifests found",
        );
    }
    let (members, declared) = workspace::read_member_manifests(ctx, &manifests, inherited);
    let by_name = index_lock(&locked);
    identity::check_declared(ctx, relative, &declared, &by_name, &members);
    graph::check_membership(ctx, relative, &locked, &members);
    graph::check_lock_graph(ctx, relative, &locked, &by_name, &members);
    graph::check_lock_mtime(ctx, relative, &manifests);
    locked
}
fn read_lock(ctx: &mut FreshnessContext, relative: &str) -> Option<Vec<Value>> {
    let lock = if relative.is_empty() {
        PathBuf::from("Cargo.lock")
    } else {
        Path::new(relative).join("Cargo.lock")
    };
    let document = read_toml_row(ctx, &lock, "lock-staleness")?;
    let Some(packages) = document.get("package").and_then(Value::as_array) else {
        ctx.fail_row(
            "lock-staleness",
            &path_text(&lock),
            "Cargo.lock has no package array",
        );
        return None;
    };
    Some(packages.clone())
}

fn index_lock(locked: &[Value]) -> BTreeMap<String, Vec<Value>> {
    let mut by_name = BTreeMap::new();
    for entry in locked {
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            by_name
                .entry(name.to_owned())
                .or_insert_with(Vec::new)
                .push(entry.clone());
        }
    }
    by_name
}
fn version(entry: &Value) -> Option<&str> {
    entry
        .get("version")
        .and_then(Value::as_str)
        .map(|value| value.split('+').next().unwrap_or(""))
}

fn source(entry: &Value) -> String {
    match entry.get("source") {
        None => "local".to_owned(),
        Some(Value::String(source)) => source.clone(),
        Some(_) => "<invalid source>".to_owned(),
    }
}

fn identity(entry: &Value) -> String {
    format!(
        "{} {} {}",
        entry.get("name").and_then(Value::as_str).unwrap_or(""),
        entry.get("version").and_then(Value::as_str).unwrap_or(""),
        source(entry)
    )
}

fn lock_graph_subject(relative: &str) -> String {
    if relative.is_empty() {
        "(lock-graph)".to_owned()
    } else {
        format!("{relative}/(lock-graph)")
    }
}
