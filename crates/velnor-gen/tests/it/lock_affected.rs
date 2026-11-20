// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::PackageName;
use velnor_rust::lock_affected;

use crate::select::name;

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

#[derive(Clone)]
pub(crate) struct Pkg {
    name: &'static str,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    dependencies: Vec<String>,
}

fn owned(dependencies: &[&str]) -> Vec<String> {
    dependencies.iter().map(|d| (*d).to_owned()).collect()
}

pub(crate) fn member(name: &'static str, dependencies: &[&str]) -> Pkg {
    Pkg {
        name,
        version: "0.0.0".to_owned(),
        source: None,
        checksum: None,
        dependencies: owned(dependencies),
    }
}

pub(crate) fn krate(name: &'static str, version: &str, dependencies: &[&str]) -> Pkg {
    Pkg {
        name,
        version: version.to_owned(),
        source: Some(REGISTRY.to_owned()),
        checksum: Some(format!("sum-{name}-{version}")),
        dependencies: owned(dependencies),
    }
}

impl Pkg {
    fn source(mut self, source: &str) -> Self {
        self.source = Some(source.to_owned());
        self
    }

    fn checksum(mut self, checksum: &str) -> Self {
        self.checksum = Some(checksum.to_owned());
        self
    }

    pub(crate) fn version(mut self, version: &str) -> Self {
        version.clone_into(&mut self.version);
        self
    }

    fn entry(&self) -> String {
        let mut lines = vec![
            format!("name = \"{}\"", self.name),
            format!("version = \"{}\"", self.version),
        ];
        lines.extend(
            self.source
                .iter()
                .map(|source| format!("source = \"{source}\"")),
        );
        lines.extend(
            self.checksum
                .iter()
                .map(|checksum| format!("checksum = \"{checksum}\"")),
        );
        if !self.dependencies.is_empty() {
            let listed: Vec<String> = self
                .dependencies
                .iter()
                .map(|dependency| format!("\"{dependency}\""))
                .collect();
            lines.push(format!("dependencies = [{}]", listed.join(", ")));
        }
        format!("\n[[package]]\n{}\n", lines.join("\n"))
    }
}

pub(crate) fn render(format: i64, packages: &[Pkg], extra: &str) -> String {
    let entries: String = packages.iter().map(Pkg::entry).collect();
    format!("version = {format}\n{entries}{extra}")
}

pub(crate) fn workspace() -> Vec<Pkg> {
    vec![
        member("app", &["lib"]),
        member("core", &["serde"]),
        member("lib", &["core", "hex"]),
        member("tool", &["fnv", "hex"]),
        krate("fnv", "1.0.5", &[]),
        krate("hex", "0.4.3", &[]),
        krate("serde", "1.0.1", &["serde_derive"]),
        krate("serde_derive", "1.0.1", &[]),
    ]
}

pub(crate) fn replaced(packages: &[Pkg], with: Pkg) -> Vec<Pkg> {
    let mut packages = packages.to_vec();
    if let Some(slot) = packages
        .iter_mut()
        .find(|package| package.name == with.name)
    {
        *slot = with;
    }
    packages
}

pub(crate) fn names() -> Vec<PackageName> {
    ["app", "core", "lib", "tool"].map(name).to_vec()
}

pub(crate) fn affected(base: &str, head: &str) -> Option<BTreeMap<String, String>> {
    affected_with(base, head, &[])
}

pub(crate) fn affected_with(
    base: &str,
    head: &str,
    edited: &[&str],
) -> Option<BTreeMap<String, String>> {
    let edited: BTreeSet<PackageName> = edited.iter().copied().map(name).collect();
    lock_affected(base, head, &names(), &edited)
        .ok()
        .map(|found| {
            found
                .into_iter()
                .map(|(member, cause)| (member.to_string(), cause))
                .collect()
        })
}

fn found(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
    rows.iter()
        .map(|(member, cause)| ((*member).to_owned(), (*cause).to_owned()))
        .collect()
}

#[test]
fn a_changed_dependency_names_every_member_that_reaches_it_and_no_other() {
    let base = workspace();
    let before = render(4, &base, "");
    let bumped = |package: Pkg| render(4, &replaced(&base, package), "");
    let rows = [
        (
            bumped(krate("fnv", "1.0.6", &[])),
            Some(found(&[("tool", "fnv")])),
        ),
        (
            bumped(krate("hex", "0.4.4", &[])),
            Some(found(&[("lib", "hex"), ("app", "hex"), ("tool", "hex")])),
        ),
        (
            bumped(krate("serde", "1.0.2", &["serde_derive"])),
            Some(found(&[
                ("core", "serde"),
                ("lib", "serde"),
                ("app", "serde"),
            ])),
        ),
        (
            bumped(krate("serde_derive", "1.0.2", &[])),
            Some(found(&[
                ("core", "serde_derive"),
                ("lib", "serde_derive"),
                ("app", "serde_derive"),
            ])),
        ),
        (
            bumped(krate("fnv", "1.0.5", &[]).checksum("other")),
            Some(found(&[("tool", "fnv")])),
        ),
        (
            bumped(krate("fnv", "1.0.5", &[]).source("sparse+https://example.com/index/")),
            Some(found(&[("tool", "fnv")])),
        ),
        (before.clone(), Some(found(&[]))),
    ];
    for (head, expected) in rows {
        assert_eq!(affected(&before, &head), expected, "{head}");
    }
}

mod edges;
