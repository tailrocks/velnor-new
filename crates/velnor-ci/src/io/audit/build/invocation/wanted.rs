// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{RustMember, Target, TargetKind};

use super::package_spec::named;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Gate {
    Open,
    Bench,
    Doc,
    Test,
}

impl Gate {
    fn allows(self, target: &Target) -> bool {
        match self {
            Self::Open => true,
            Self::Bench => target.bench,
            Self::Doc => target.doc,
            Self::Test => {
                target.tests
                    || (target.doctest
                        && target
                            .kinds
                            .iter()
                            .any(|kind| matches!(kind, TargetKind::Lib | TargetKind::ProcMacro)))
            }
        }
    }
}

pub(super) struct Wanted<'a> {
    pub(super) kinds: Vec<(TargetKind, Option<&'a str>, Gate)>,
    pub(super) harnesses: bool,
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

pub(super) fn wanted<'a>(arguments: &[&str], words: &[&'a str]) -> Wanted<'a> {
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
                    TargetKind::Example,
                ]
                .map(|kind| (kind, None, Gate::Open)),
            );
            kinds.push((TargetKind::Bench, None, Gate::Bench));
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
            ["test" | "t", ..] => Gate::Test,
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

pub(super) fn compiles(
    member: &RustMember,
    wanted: &Wanted<'_>,
    all_features: bool,
    enabled: &BTreeSet<String>,
) -> bool {
    member.targets.iter().any(|target| {
        (if names_known(target) {
            target
                .required_features
                .iter()
                .all(|feature| enabled.contains(&feature.name))
        } else {
            all_features
        }) && (wanted.harnesses && target.tests
            || wanted.kinds.iter().any(|(kind, name, gate)| {
                target.kinds.contains(kind)
                    && gate.allows(target)
                    && (name.is_some() || *kind != TargetKind::Test || target.tests)
                    && name.is_none_or(|name| named(name, target.name.as_str()))
            }))
    })
}

fn names_known(target: &Target) -> bool {
    !target.requires_features || !target.required_features.is_empty()
}

pub(super) fn runs_doctests(arguments: &[&str], words: &[&str]) -> bool {
    matches!(arguments, ["test" | "t", ..])
        && !words.iter().any(|word| {
            matches!(
                *word,
                "--all-targets" | "--tests" | "--lib" | "--bins" | "--examples" | "--benches"
            ) || flag(word, &["--test", "--bin", "--example", "--bench"])
        })
}

pub(super) fn flag(word: &str, names: &[&str]) -> bool {
    names.iter().any(|name| {
        word == *name
            || word
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('='))
    })
}
