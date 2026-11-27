// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use toml::Value;
use velnor_model::{RepoPath, RepoView};

use crate::detect::GENERATOR_PACKAGE;
use crate::roots::CARGO_LOCK;
use crate::toml_file::table;

pub(crate) const EXTERNALS: RepoPath = RepoPath::from_static(".velnor/generator-externals.lock");
const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

type Id<'a> = (&'a str, &'a str, Option<&'a str>);

struct Locked<'a> {
    id: Id<'a>,
    dependencies: Vec<&'a str>,
    replaced: bool,
}

fn packages(lock: &toml::Table) -> Vec<Locked<'_>> {
    lock.get("package")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|package| {
            Some(Locked {
                id: (
                    package.get("name")?.as_str()?,
                    package.get("version")?.as_str()?,
                    package.get("source").and_then(Value::as_str),
                ),
                dependencies: package
                    .get("dependencies")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect(),
                replaced: package.get("replace").is_some(),
            })
        })
        .collect()
}

fn resolve<'a, 'b>(packages: &'b [Locked<'a>], dependency: &str) -> Vec<&'b Locked<'a>> {
    let mut parts = dependency.splitn(3, ' ');
    let name = parts.next().unwrap_or_default();
    let version = parts.next();
    let source = parts
        .next()
        .map(|source| source.trim_start_matches('(').trim_end_matches(')'));
    let named: Vec<&Locked<'a>> = packages
        .iter()
        .filter(|candidate| {
            candidate.id.0 == name
                && version.is_none_or(|version| candidate.id.1 == version)
                && source.is_none_or(|source| candidate.id.2 == Some(source))
        })
        .collect();
    let local: Vec<&Locked<'a>> = named
        .iter()
        .copied()
        .filter(|candidate| candidate.id.2.is_none())
        .collect();
    if source.is_none() && !local.is_empty() {
        local
    } else {
        named
    }
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
    packages.iter().any(|package| {
        package.replaced
            || package
                .id
                .2
                .is_some_and(|source| !CRATES_IO.contains(&source))
    }) || unlisted(view, &packages)
}

fn listed(view: &RepoView) -> BTreeSet<String> {
    view.files
        .get(&EXTERNALS)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .into_iter()
        .flat_map(str::lines)
        .map(str::to_owned)
        .collect()
}

fn reached(packages: &[Locked<'_>]) -> BTreeSet<String> {
    let mut pending: Vec<&Locked<'_>> = packages
        .iter()
        .filter(|package| package.id.0 == GENERATOR_PACKAGE && package.id.2.is_none())
        .collect();
    let mut seen: BTreeSet<Id<'_>> = BTreeSet::new();
    while let Some(package) = pending.pop() {
        if !seen.insert(package.id) {
            continue;
        }
        for dependency in &package.dependencies {
            pending.extend(resolve(packages, dependency));
        }
    }
    seen.iter()
        .filter_map(|(name, version, source)| Some(format!("{name} {version} {}", (*source)?)))
        .collect()
}

fn unlisted(view: &RepoView, packages: &[Locked<'_>]) -> bool {
    !reached(packages).is_subset(&listed(view))
}

#[must_use]
pub fn externals_drift(view: &RepoView, manifest: &RepoPath) -> Option<(Vec<String>, Vec<String>)> {
    let lock = table(view.files.get(&manifest.sibling(&CARGO_LOCK))?).ok()?;
    let reached = reached(&packages(&lock));
    let listed = listed(view);
    Some((
        reached.difference(&listed).cloned().collect(),
        listed.difference(&reached).cloned().collect(),
    ))
}
