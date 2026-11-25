// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use velnor_model::{RepoPath, RustMember, Target, TargetKind, UnownedPackage};

use super::package_spec::{Spec, named, parse};
use super::platform::{Platform, Which};
use super::resolve::{Request, Resolver, tests_of};
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
        probe: &dyn Fn(Which<'_>) -> Option<Platform>,
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
        let dev = with_dev_targets(arguments, &words);
        let all_features = words.contains(&"--all-features");
        let defaults = !words.contains(&"--no-default-features");
        let features = feature_names(&words);
        let target = target_flag(&words);
        let host = OnceCell::new();
        let aimed = OnceCell::new();
        let platform = |build: bool| {
            if build {
                host.get_or_init(|| probe(Which::Host)).as_ref()
            } else {
                aimed.get_or_init(|| probe(Which::Target(target))).as_ref()
            }
        };
        let explicit = dev
            && words
                .iter()
                .any(|word| matches!(*word, "--lib" | "--bins") || flag(word, &["--bin"]));
        let wanted = wanted(arguments, &words);
        let workspace = words
            .iter()
            .any(|word| matches!(*word, "--workspace" | "--all"));
        let (selected, excluded, provable) = named_packages(&words);
        let selected: Vec<Spec<'_>> = selected.into_iter().filter_map(parse).collect();
        let excluded: Vec<Spec<'_>> = excluded.into_iter().filter_map(parse).collect();
        let candidates: Vec<&RustMember> = members
            .iter()
            .filter(|member| {
                provable
                    && if workspace {
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
        let request = Request {
            all: all_features,
            defaults,
            names: &features,
        };
        let mut own = Resolver::new(members, unowned, &platform);
        for member in &candidates {
            own.mark(member, dev && (explicit || tests_of(member)));
        }
        for member in &candidates {
            own.start(member, &request);
        }
        let chosen: Vec<&RustMember> = candidates
            .into_iter()
            .filter(|member| {
                compiles(
                    member,
                    &wanted,
                    all_features,
                    &own.enabled(&member.manifest),
                )
            })
            .collect();
        let mut built = Resolver::new(members, unowned, &platform);
        for member in &chosen {
            built.mark(member, dev && (explicit || tests_of(member)));
        }
        for member in &chosen {
            built.start(member, &request);
        }
        let mut packages: Vec<String> = chosen
            .iter()
            .map(|member| member.name.as_str().to_owned())
            .collect();
        packages.extend(built.packages().into_iter().map(str::to_owned));
        Some(Selection {
            root: root.to_owned(),
            packages,
        })
    }
}

fn with_dev_targets(arguments: &[&str], words: &[&str]) -> bool {
    matches!(
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
    })
}

fn named_packages<'a>(words: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>, bool) {
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
    (selected, excluded, provable)
}

fn target_flag<'a>(words: &[&'a str]) -> Option<&'a str> {
    words.iter().enumerate().find_map(|(index, word)| {
        word.strip_prefix("--target=").or_else(|| {
            (*word == "--target")
                .then(|| words.get(index + 1).copied())
                .flatten()
        })
    })
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

#[derive(Clone, Copy, PartialEq)]
enum Gate {
    Open,
    Bench,
    Doc,
}

impl Gate {
    fn allows(self, target: &Target) -> bool {
        match self {
            Self::Open => true,
            Self::Bench => target.bench,
            Self::Doc => target.doc,
        }
    }
}

struct Wanted<'a> {
    kinds: Vec<(TargetKind, Option<&'a str>, Gate)>,
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
    let mut kinds: Vec<(TargetKind, Option<&str>, Gate)> = Vec::new();
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
                .map(|kind| (kind, None, Gate::Open)),
            );
            harnesses = true;
        } else if flag(word, &["--lib"]) {
            kinds.extend(LIBRARIES.map(|kind| (kind, None, Gate::Open)));
        } else if *word == "--bins" {
            kinds.push((TargetKind::Bin, None, Gate::Open));
        } else if flag(word, &["--bin"]) {
            kinds.push((
                TargetKind::Bin,
                target_name(words, index, "--bin"),
                Gate::Open,
            ));
        } else if *word == "--examples" {
            kinds.push((TargetKind::Example, None, Gate::Open));
        } else if flag(word, &["--example"]) {
            kinds.push((
                TargetKind::Example,
                target_name(words, index, "--example"),
                Gate::Open,
            ));
        } else if *word == "--tests" {
            harnesses = true;
        } else if flag(word, &["--test"]) {
            kinds.push((
                TargetKind::Test,
                target_name(words, index, "--test"),
                Gate::Open,
            ));
        } else if *word == "--benches" {
            kinds.extend(BENCHED.map(|kind| (kind, None, Gate::Bench)));
        } else if flag(word, &["--bench"]) {
            kinds.push((
                TargetKind::Bench,
                target_name(words, index, "--bench"),
                Gate::Open,
            ));
        }
    }
    if kinds.is_empty() {
        let gate = match arguments {
            ["bench", ..] => Gate::Bench,
            ["doc" | "d", ..] => Gate::Doc,
            _ => Gate::Open,
        };
        kinds.extend(
            [TargetKind::Lib, TargetKind::ProcMacro, TargetKind::Bin]
                .map(|kind| (kind, None, gate)),
        );
        match arguments {
            ["test" | "t" | "nextest", ..] => {
                kinds.extend(
                    [TargetKind::Test, TargetKind::Example].map(|kind| (kind, None, Gate::Open)),
                );
            }
            ["bench", ..] => kinds.push((TargetKind::Bench, None, Gate::Bench)),
            _ => {}
        }
    }
    Wanted { kinds, harnesses }
}

fn compiles(
    member: &RustMember,
    wanted: &Wanted<'_>,
    all_features: bool,
    enabled: &BTreeSet<String>,
) -> bool {
    member.targets.iter().any(|target| {
        (all_features
            || (names_known(target)
                && target
                    .required_features
                    .iter()
                    .all(|feature| enabled.contains(&feature.name))))
            && (wanted.harnesses && target.tests
                || wanted.kinds.iter().any(|(kind, name, gate)| {
                    target.kinds.contains(kind)
                        && gate.allows(target)
                        && (name.is_some() || *kind != TargetKind::Test || target.tests)
                        && name.is_none_or(|name| named(name, target.name.as_str()))
                }))
    })
}

/// A target the recorded file marks as gated without naming the features (an
/// older pin) cannot be proven enabled.
fn names_known(target: &Target) -> bool {
    !target.requires_features || !target.required_features.is_empty()
}

fn flag(word: &str, names: &[&str]) -> bool {
    names.iter().any(|name| {
        word == *name
            || word
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('='))
    })
}
