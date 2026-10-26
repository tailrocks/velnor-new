// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::metadata_edges::{DepKind, LocalEdge};

pub fn manifest_edges(
    text: &str,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
) -> Result<Vec<LocalEdge>, String> {
    let document: toml::Table = toml::from_str(text).map_err(|err| err.to_string())?;
    let mut edges = Vec::new();
    for (table, kind) in sections() {
        if let Some(deps) = document.get(table).and_then(toml::Value::as_table) {
            edges.extend(dep_edges(deps, from, dir, packages, kind, None));
        }
    }
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for (name, target) in targets {
            let Some(target) = target.as_table() else {
                continue;
            };
            for (table, kind) in sections() {
                if let Some(deps) = target.get(table).and_then(toml::Value::as_table) {
                    edges.extend(dep_edges(deps, from, dir, packages, kind, Some(name)));
                }
            }
        }
    }
    Ok(edges)
}

fn sections() -> [(&'static str, DepKind); 3] {
    [
        ("dependencies", DepKind::Normal),
        ("build-dependencies", DepKind::Build),
        ("dev-dependencies", DepKind::Dev),
    ]
}

fn dep_edges(
    deps: &toml::Table,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
    kind: DepKind,
    target: Option<&str>,
) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for spec in deps.values() {
        let Some(spec) = spec.as_table() else {
            continue;
        };
        let Some(path) = spec.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let joined = join_dir(dir, path);
        let Some(to) = packages
            .iter()
            .find(|(owned, _)| *owned == joined)
            .map(|(_, id)| id)
        else {
            continue;
        };
        let optional = spec
            .get("optional")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        edges.push(LocalEdge {
            from: from.to_owned(),
            to: to.clone(),
            kind,
            optional,
            target: target.map(str::to_owned),
        });
    }
    edges
}

fn join_dir(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|seg| !seg.is_empty()).collect();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}
