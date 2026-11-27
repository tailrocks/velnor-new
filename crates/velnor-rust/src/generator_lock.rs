// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use toml::Value;
use velnor_model::{RepoPath, RepoView};

use crate::detect::GENERATOR_PACKAGE;
use crate::roots::CARGO_LOCK;
use crate::toml_file::table;

pub(crate) const EXTERNALS: RepoPath = RepoPath::from_static(".velnor/generator-externals.lock");
const REGISTRIES: [&str; 2] = ["registry+", "sparse+"];

struct Locked<'a> {
    name: &'a str,
    version: &'a str,
    source: Option<&'a str>,
    dependencies: Vec<&'a str>,
}

fn packages(lock: &toml::Table) -> Vec<Locked<'_>> {
    lock.get("package")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| {
            Some(Locked {
                name: package.get("name")?.as_str()?,
                version: package.get("version")?.as_str()?,
                source: package.get("source").and_then(Value::as_str),
                dependencies: package
                    .get("dependencies")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect(),
            })
        })
        .collect()
}

pub(crate) fn unmodelled(view: &RepoView, manifest: &RepoPath) -> bool {
    let Some(lock) = view
        .files
        .get(&manifest.sibling(&CARGO_LOCK))
        .and_then(|bytes| table(bytes).ok())
    else {
        return true;
    };
    let packages = packages(&lock);
    packages
        .iter()
        .filter_map(|package| package.source)
        .any(|source| !REGISTRIES.iter().any(|scheme| source.starts_with(scheme)))
        || unlisted(view, &packages)
}

fn unlisted(view: &RepoView, packages: &[Locked<'_>]) -> bool {
    let listed: BTreeSet<&str> = view
        .files
        .get(&EXTERNALS)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .into_iter()
        .flat_map(str::lines)
        .collect();
    let mut pending: Vec<&Locked<'_>> = packages
        .iter()
        .filter(|package| package.name == GENERATOR_PACKAGE && package.source.is_none())
        .collect();
    let mut reached: BTreeMap<(&str, &str), bool> = BTreeMap::new();
    while let Some(package) = pending.pop() {
        if reached
            .insert((package.name, package.version), package.source.is_some())
            .is_some()
        {
            continue;
        }
        for dependency in &package.dependencies {
            let mut parts = dependency.split(' ');
            let name = parts.next().unwrap_or_default();
            let version = parts.next();
            pending.extend(packages.iter().filter(|candidate| {
                candidate.name == name && version.is_none_or(|version| candidate.version == version)
            }));
        }
    }
    reached
        .iter()
        .filter(|(_, external)| **external)
        .any(|((name, version), _)| !listed.contains(format!("{name} {version}").as_str()))
}
