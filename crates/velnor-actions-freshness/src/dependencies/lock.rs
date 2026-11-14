// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod graph;

mod identity;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use std::path::{Path, PathBuf};

use toml::Value;

use crate::context::FreshnessContext;

use super::{
    MemberOwners, MemberWorkspace, manifest_label, path_dependencies, path_text, read_toml_row,
    workspace,
};

pub(super) fn check_workspace(
    ctx: &mut FreshnessContext,
    relative: &str,
    document: &Value,
    owners: &MemberOwners,
    manifests: &[PathBuf],
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
    if manifests.is_empty() {
        ctx.fail_row(
            "lock-staleness",
            &label,
            "no workspace member manifests found",
        );
    }
    let inherited_base = ctx.root.join(relative);
    let (members, declared) =
        workspace::read_member_manifests(ctx, manifests, inherited, &inherited_base);
    let by_name = index_lock(&locked);
    identity::check_declared(ctx, relative, &declared, &by_name, &members, owners);
    let expected = transitive_path_packages(ctx, relative, document, owners, manifests);
    graph::check_membership(ctx, relative, &locked, &expected);
    graph::check_lock_graph(ctx, relative, &locked, &by_name, &members);
    graph::check_lock_mtime(ctx, relative, manifests);
    locked
}

fn transitive_path_packages(
    ctx: &mut FreshnessContext,
    relative: &str,
    document: &Value,
    owners: &MemberOwners,
    manifests: &[PathBuf],
) -> BTreeSet<(String, String)> {
    let mut path_packages = BTreeSet::new();
    let root_workspace = MemberWorkspace {
        root: ctx.root.join(relative),
        document: document.clone(),
    };
    let mut pending = manifests
        .iter()
        .cloned()
        .map(|manifest| (None::<String>, manifest, root_workspace.clone()))
        .collect::<VecDeque<_>>();
    let mut visited = BTreeSet::new();
    while let Some((expected_name, manifest, workspace)) = pending.pop_front() {
        let Ok(manifest) = manifest.canonicalize() else {
            continue;
        };
        let Some(target) = read_toml_row(ctx, &manifest, "lock-staleness") else {
            continue;
        };
        let name = target
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str);
        if expected_name
            .as_deref()
            .is_some_and(|expected| name != Some(expected))
        {
            continue;
        }
        let package = target.get("package").and_then(Value::as_table);
        let actual = package
            .and_then(|package| identity::effective_package_version(package, &workspace.document));
        if let (Some(name), Some(actual)) = (name, actual) {
            path_packages.insert((name.to_owned(), normalize_version(&actual).to_owned()));
        } else {
            ctx.fail_row(
                "lock-staleness",
                &path_text(&manifest),
                "local package has no effective name and version",
            );
        }
        if !visited.insert(manifest.clone()) {
            continue;
        }
        for (name, target) in path_dependencies::path_dependencies(
            &target,
            manifest.parent().unwrap_or(Path::new(".")),
            &workspace,
        ) {
            let Ok(target) = target.canonicalize() else {
                continue;
            };
            if let Some(owner) = owners
                .get(&target)
                .filter(|owners| owners.len() == 1)
                .and_then(|owners| owners.first())
            {
                pending.push_back((Some(name), target, owner.clone()));
            }
        }
    }
    path_packages
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
        .map(normalize_version)
}

fn normalize_version(value: &str) -> &str {
    value.split('+').next().unwrap_or("")
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
