// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::{Table, Value};

use super::changes::git;

const SECTIONS: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];

#[derive(Debug, Clone)]
pub(super) struct Unit {
    pub(super) dir: String,
    pub(super) name: String,
    pub(super) workspace: String,
    pub(super) build_script: bool,
}

#[derive(Debug, Default)]
pub(super) struct Units {
    pub(super) units: Vec<Unit>,
    pub(super) edges: Vec<(String, String)>,
}

pub(super) fn load(
    root: &Path,
    routed: &BTreeSet<String>,
    tracked: &BTreeSet<String>,
) -> Result<Units, String> {
    let mut dirs: BTreeSet<String> = routed.clone();
    let mut roots = BTreeSet::new();
    for dir in routed {
        roots.insert(workspace_root(root, dir)?);
    }
    for workspace in &roots {
        dirs.extend(members(root, workspace, tracked)?);
    }
    let mut manifests: BTreeMap<&str, Table> = BTreeMap::new();
    for dir in &dirs {
        manifests.insert(dir, manifest(&root.join(dir).join("Cargo.toml"))?);
    }
    let mut units = Vec::new();
    for (dir, table) in &manifests {
        let name = table
            .get("package")
            .and_then(Value::as_table)
            .and_then(|package| package.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("unnamed_package:{dir}"))?;
        units.push(Unit {
            dir: (*dir).to_owned(),
            name: name.to_owned(),
            workspace: workspace_root(root, dir)?,
            build_script: has_build_script(dir, table, tracked),
        });
    }
    let mut edges = Vec::new();
    for unit in &units {
        let Some(table) = manifests.get(unit.dir.as_str()) else {
            continue;
        };
        let inherited = inherited_paths(root, &unit.workspace)?;
        for path in dependency_paths(table, &inherited, &unit.dir) {
            if dirs.contains(&path) {
                edges.push((unit.dir.clone(), path));
            }
        }
    }
    Ok(Units { units, edges })
}

fn members(
    root: &Path,
    workspace: &str,
    tracked: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let table = manifest(&root.join(workspace).join("Cargo.toml"))?;
    let Some(section) = table.get("workspace").and_then(Value::as_table) else {
        return Ok(BTreeSet::from([workspace.to_owned()]));
    };
    let patterns = |key: &str| -> Vec<String> {
        section
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|pattern| join(workspace, pattern))
            .collect()
    };
    let (include, exclude) = (patterns("members"), patterns("exclude"));
    let mut found = BTreeSet::new();
    if table.contains_key("package") {
        found.insert(workspace.to_owned());
    }
    for path in tracked {
        let Some(dir) = path.strip_suffix("/Cargo.toml") else {
            continue;
        };
        if include
            .iter()
            .any(|pattern| super::glob::matches(pattern, dir))
            && !exclude
                .iter()
                .any(|pattern| super::glob::matches(pattern, dir))
        {
            found.insert(dir.to_owned());
        }
    }
    Ok(found)
}

pub(super) fn base_edges(
    root: &Path,
    base: &str,
    units: &[Unit],
    added: &BTreeSet<String>,
) -> Result<Vec<(String, String)>, String> {
    let dirs: BTreeSet<&str> = units.iter().map(|unit| unit.dir.as_str()).collect();
    let mut edges = Vec::new();
    for unit in units {
        let manifest = format!("{}/Cargo.toml", unit.dir);
        if added.contains(&manifest) {
            continue;
        }
        let object = format!("{base}:{manifest}");
        let text =
            String::from_utf8(git(root, &["show", &object])?).map_err(|error| error.to_string())?;
        let table: Table = toml::from_str(&text).map_err(|error| error.to_string())?;
        for (_, path) in dependency_specs(&table, &unit.dir) {
            if let Some(path) = path.filter(|path| dirs.contains(path.as_str())) {
                edges.push((unit.dir.clone(), path));
            }
        }
    }
    Ok(edges)
}

fn manifest(path: &Path) -> Result<Table, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn workspace_root(root: &Path, dir: &str) -> Result<String, String> {
    let mut current = dir;
    loop {
        let path = root.join(current).join("Cargo.toml");
        if path.is_file() && manifest(&path)?.contains_key("workspace") {
            return Ok(current.to_owned());
        }
        match current.rsplit_once('/') {
            Some((parent, _)) => current = parent,
            None if current.is_empty() => return Ok(dir.to_owned()),
            None => current = "",
        }
    }
}

fn has_build_script(dir: &str, table: &Table, tracked: &BTreeSet<String>) -> bool {
    let package = table.get("package").and_then(Value::as_table);
    match package.and_then(|package| package.get("build")) {
        Some(Value::Boolean(false)) => false,
        Some(Value::String(_) | Value::Boolean(true)) => true,
        _ => tracked.contains(&join(dir, "build.rs")),
    }
}

fn inherited_paths(root: &Path, workspace: &str) -> Result<BTreeMap<String, String>, String> {
    let path = root.join(workspace).join("Cargo.toml");
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let table = manifest(&path)?;
    Ok(table
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(name, spec)| {
            let path = spec.as_table()?.get("path")?.as_str()?;
            Some((name.clone(), join(workspace, path)))
        })
        .collect())
}

fn dependency_paths(
    table: &Table,
    inherited: &BTreeMap<String, String>,
    dir: &str,
) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for (name, path) in dependency_specs(table, dir) {
        match path {
            Some(path) => {
                paths.insert(path);
            }
            None => {
                if let Some(path) = name.and_then(|name| inherited.get(&name)) {
                    paths.insert(path.clone());
                }
            }
        }
    }
    paths
}

fn dependency_specs(table: &Table, dir: &str) -> Vec<(Option<String>, Option<String>)> {
    let mut found = Vec::new();
    let mut tables: Vec<&Table> = vec![table];
    tables.extend(
        table
            .get("target")
            .and_then(Value::as_table)
            .into_iter()
            .flat_map(|targets| targets.values())
            .filter_map(Value::as_table),
    );
    for table in tables {
        for section in SECTIONS {
            for (name, spec) in table
                .get(section)
                .and_then(Value::as_table)
                .into_iter()
                .flatten()
            {
                let Some(spec) = spec.as_table() else {
                    continue;
                };
                if let Some(path) = spec.get("path").and_then(Value::as_str) {
                    found.push((None, Some(join(dir, path))));
                } else if spec.get("workspace").and_then(Value::as_bool) == Some(true) {
                    found.push((Some(name.clone()), None));
                }
            }
        }
    }
    found
}

pub(super) fn join(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|seg| !seg.is_empty()).collect();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
