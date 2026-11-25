// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use velnor_model::{
    DependencyKind, MemberDependency, RepoPath, RustMember, TargetKind, UnownedPackage,
};

use super::package_spec::{Spec, named, parse};
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
        let all_features = words.contains(&"--all-features");
        let features = feature_names(&words);
        let wanted = wanted(arguments, &words);
        let workspace = words
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
                if !provable || !compiles(member, &wanted, all_features) {
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
            built(&chosen, members, unowned, dev, all_features, &features)
                .into_iter()
                .map(str::to_owned),
        );
        Some(Selection {
            root: root.to_owned(),
            packages,
        })
    }
}

fn cluster_flag(word: &str, letter: char) -> Option<&str> {
    let flags = word.strip_prefix('-')?;
    if flags.starts_with('-') {
        return None;
    }
    flags
        .trim_start_matches(['q', 'v', 'r'])
        .strip_prefix(letter)
}

fn cluster_package(word: &str) -> Option<&str> {
    cluster_flag(word, 'p')
}

fn feature_names<'a>(words: &[&'a str]) -> Vec<&'a str> {
    let mut names = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let value = if *word == "--features" {
            words.get(index + 1).copied()
        } else if let Some(value) = word.strip_prefix("--features=") {
            Some(value)
        } else {
            cluster_flag(word, 'F').and_then(|rest| {
                if rest.is_empty() {
                    words.get(index + 1).copied()
                } else {
                    Some(rest.strip_prefix('=').unwrap_or(rest))
                }
            })
        };
        names.extend(
            value
                .into_iter()
                .flat_map(|value| value.split([',', ' ']))
                .filter(|name| !name.is_empty()),
        );
    }
    names
}

fn enables(features: &[&str], member: &RustMember, dependency: &MemberDependency) -> bool {
    let name = dependency.feature.as_str();
    features.iter().any(|feature| {
        if *feature == name || feature.strip_prefix("dep:") == Some(name) {
            return true;
        }
        let Some((left, right)) = feature.split_once('/') else {
            return false;
        };
        left == name
            || left == member.name.as_str()
                && (right == name || right.strip_prefix("dep:") == Some(name))
    })
}

struct Wanted<'a> {
    kinds: Vec<(TargetKind, Option<&'a str>)>,
    harnesses: bool,
}

fn target_name<'a>(words: &[&'a str], index: usize, name: &str) -> Option<&'a str> {
    let word = words[index];
    word.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('='))
        .or_else(|| {
            (word == name)
                .then(|| words.get(index + 1).copied())
                .flatten()
                .filter(|value| !value.starts_with('-'))
        })
}

fn wanted<'a>(arguments: &[&str], words: &[&'a str]) -> Wanted<'a> {
    const LIBRARIES: [TargetKind; 2] = [TargetKind::Lib, TargetKind::ProcMacro];
    const BENCHED: [TargetKind; 4] = [
        TargetKind::Lib,
        TargetKind::ProcMacro,
        TargetKind::Bin,
        TargetKind::Bench,
    ];
    let mut kinds: Vec<(TargetKind, Option<&str>)> = Vec::new();
    let mut harnesses = false;
    for (index, word) in words.iter().enumerate() {
        if *word == "--all-targets" {
            kinds.extend(
                [
                    TargetKind::Lib,
                    TargetKind::ProcMacro,
                    TargetKind::Bin,
                    TargetKind::Test,
                    TargetKind::Bench,
                    TargetKind::Example,
                ]
                .map(|kind| (kind, None)),
            );
            harnesses = true;
        } else if flag(word, &["--lib"]) {
            kinds.extend(LIBRARIES.map(|kind| (kind, None)));
        } else if *word == "--bins" {
            kinds.push((TargetKind::Bin, None));
        } else if flag(word, &["--bin"]) {
            kinds.push((TargetKind::Bin, target_name(words, index, "--bin")));
        } else if *word == "--examples" {
            kinds.push((TargetKind::Example, None));
        } else if flag(word, &["--example"]) {
            kinds.push((TargetKind::Example, target_name(words, index, "--example")));
        } else if *word == "--tests" {
            harnesses = true;
        } else if flag(word, &["--test"]) {
            kinds.push((TargetKind::Test, target_name(words, index, "--test")));
        } else if *word == "--benches" {
            kinds.extend(BENCHED.map(|kind| (kind, None)));
        } else if flag(word, &["--bench"]) {
            kinds.push((TargetKind::Bench, target_name(words, index, "--bench")));
        }
    }
    if kinds.is_empty() {
        kinds.extend(
            [TargetKind::Lib, TargetKind::ProcMacro, TargetKind::Bin].map(|kind| (kind, None)),
        );
        match arguments {
            ["test" | "t" | "nextest", ..] => {
                kinds.extend([TargetKind::Test, TargetKind::Example].map(|kind| (kind, None)));
            }
            ["bench", ..] => kinds.push((TargetKind::Bench, None)),
            _ => {}
        }
    }
    Wanted { kinds, harnesses }
}

fn compiles(member: &RustMember, wanted: &Wanted<'_>, all_features: bool) -> bool {
    member.targets.iter().any(|target| {
        (all_features || !target.requires_features)
            && (wanted.harnesses && target.tests
                || wanted.kinds.iter().any(|(kind, name)| {
                    target.kinds.contains(kind)
                        && (name.is_some() || *kind != TargetKind::Test || target.tests)
                        && name.is_none_or(|name| named(name, target.name.as_str()))
                }))
    })
}

fn built<'a>(
    chosen: &[&'a RustMember],
    members: &'a [RustMember],
    unowned: &'a BTreeMap<RepoPath, UnownedPackage>,
    dev: bool,
    all_features: bool,
    features: &[&str],
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
                .filter(move |dependency| {
                    !dependency.optional || all_features || enables(features, member, dependency)
                })
        })
        .collect();
    let mut visited = BTreeSet::new();
    let mut packages = BTreeSet::new();
    while let Some(dependency) = pending.pop() {
        if dependency.platform.is_some() || !visited.insert(&dependency.manifest) {
            continue;
        }
        let next = if let Some(package) = unowned.get(&dependency.manifest) {
            packages.insert(package.name.as_str());
            &package.dependencies
        } else if let Some(member) = members
            .iter()
            .find(|member| member.manifest == dependency.manifest)
        {
            packages.insert(member.name.as_str());
            &member.dependencies
        } else {
            continue;
        };
        pending.extend(
            next.iter().filter(|dependency| {
                dependency.kind != DependencyKind::Dev && !dependency.optional
            }),
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
