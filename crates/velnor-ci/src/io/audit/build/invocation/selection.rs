// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use velnor_model::{
    DependencyKind, MemberDependency, RepoPath, RustMember, TargetKind, UnownedPackage,
};

use super::package_spec::{Spec, parse};
use super::{Invocation, subcommand};

#[derive(Default, Serialize, Deserialize)]
pub(in super::super) struct Selection {
    #[serde(default)]
    root: String,
    packages: Vec<String>,
}

impl Selection {
    pub(in super::super) fn selects(&self, package: &str) -> bool {
        self.packages.iter().any(|selected| selected == package)
    }

    pub(in super::super) fn is_root(&self, root: &str) -> bool {
        self.root == root
    }
}

impl Invocation {
    pub(in super::super) fn selection(
        &self,
        root: &str,
        members: &[RustMember],
        unowned: &BTreeMap<RepoPath, UnownedPackage>,
        repositories: &[&Path],
    ) -> Option<Selection> {
        if !self.cargo_program() {
            return None;
        }
        let words: Vec<&str> = self
            .command
            .iter()
            .skip(1)
            .map(String::as_str)
            .take_while(|word| *word != "--")
            .collect();
        let arguments = if Path::new(&self.command[0])
            .file_name()
            .is_some_and(|name| name == "cargo")
        {
            subcommand(&words).map_or(&[][..], |start| &words[start..])
        } else {
            &words[..]
        };
        let dev = matches!(
            arguments,
            ["test" | "t" | "bench", ..] | ["nextest", "run" | "archive", ..]
        ) || words.iter().any(|word| {
            *word == "--all-targets"
                || flag(
                    word,
                    &[
                        "--tests",
                        "--test",
                        "--benches",
                        "--bench",
                        "--examples",
                        "--example",
                    ],
                )
        });
        let workspace = !words.iter().any(|word| target_filter(word))
            && words
                .iter()
                .any(|word| matches!(*word, "--workspace" | "--all"));
        let mut selected = Vec::new();
        let mut excluded = Vec::new();
        let mut provable = true;
        for (index, word) in words.iter().enumerate() {
            let (specs, spec) = if *word == "--package" {
                (&mut selected, words.get(index + 1).copied())
            } else if *word == "--exclude" {
                (&mut excluded, words.get(index + 1).copied())
            } else if let Some(spec) = word.strip_prefix("--package=") {
                (&mut selected, Some(spec))
            } else if let Some(spec) = word.strip_prefix("--exclude=") {
                (&mut excluded, Some(spec))
            } else if let Some(rest) = cluster_package(word) {
                if rest.is_empty() {
                    (&mut selected, words.get(index + 1).copied())
                } else {
                    (&mut selected, Some(rest.strip_prefix('=').unwrap_or(rest)))
                }
            } else {
                continue;
            };
            match spec {
                Some(spec) if !spec.is_empty() && !spec.starts_with('-') => specs.push(spec),
                _ => provable = false,
            }
        }
        let selected: Vec<Spec<'_>> = selected.into_iter().filter_map(parse).collect();
        let excluded: Vec<Spec<'_>> = excluded.into_iter().filter_map(parse).collect();
        let chosen: Vec<&RustMember> = members
            .iter()
            .filter(|member| {
                if !provable {
                    false
                } else if workspace {
                    !excluded
                        .iter()
                        .any(|spec| spec.matches(member, repositories))
                } else {
                    selected
                        .iter()
                        .any(|spec| spec.matches(member, repositories))
                }
            })
            .collect();
        let mut packages: Vec<String> = chosen
            .iter()
            .map(|member| member.name.as_str().to_owned())
            .collect();
        packages.extend(
            built(&chosen, members, unowned, dev)
                .into_iter()
                .map(str::to_owned),
        );
        Some(Selection {
            root: root.to_owned(),
            packages,
        })
    }
}

fn cluster_package(word: &str) -> Option<&str> {
    let flags = word.strip_prefix('-')?;
    if flags.starts_with('-') {
        return None;
    }
    flags.trim_start_matches(['q', 'v', 'r']).strip_prefix('p')
}

fn built<'a>(
    chosen: &[&'a RustMember],
    members: &'a [RustMember],
    unowned: &'a BTreeMap<RepoPath, UnownedPackage>,
    dev: bool,
) -> BTreeSet<&'a str> {
    let mut pending: Vec<&MemberDependency> = chosen
        .iter()
        .flat_map(|member| {
            let tested = dev
                && member.targets.iter().any(|target| {
                    target.tests
                        || target.kinds.iter().any(|kind| {
                            matches!(
                                kind,
                                TargetKind::Test | TargetKind::Bench | TargetKind::Example
                            )
                        })
                });
            member
                .dependencies
                .iter()
                .filter(move |dependency| tested || dependency.kind != DependencyKind::Dev)
        })
        .collect();
    let mut visited = BTreeSet::new();
    let mut packages = BTreeSet::new();
    while let Some(dependency) = pending.pop() {
        if dependency.optional
            || dependency.platform.is_some()
            || !visited.insert(&dependency.manifest)
        {
            continue;
        }
        let next = if let Some(package) = unowned.get(&dependency.manifest) {
            packages.insert(package.name.as_str());
            &package.dependencies
        } else if let Some(member) = members
            .iter()
            .find(|member| member.manifest == dependency.manifest)
        {
            &member.dependencies
        } else {
            continue;
        };
        pending.extend(
            next.iter()
                .filter(|dependency| dependency.kind != DependencyKind::Dev),
        );
    }
    packages
}

fn flag(word: &str, names: &[&str]) -> bool {
    names.iter().any(|name| {
        word == *name
            || word
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('='))
    })
}

fn target_filter(word: &str) -> bool {
    const FILTERS: [&str; 9] = [
        "--lib",
        "--bins",
        "--bin",
        "--examples",
        "--example",
        "--tests",
        "--test",
        "--benches",
        "--bench",
    ];
    flag(word, &FILTERS)
}
