// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use velnor_model::{RepoPath, RustMember, UnownedPackage};

use super::package_spec::{Spec, parse};
use super::platform::{Configured, Platform, Which, command_target};
use super::resolve::{Request, Resolver, tests_of};
use super::wanted::{compiles, flag, runs_doctests, wanted};
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
        let doctests = runs_doctests(arguments, &words);
        let all_features = words.contains(&"--all-features");
        let defaults = !words.contains(&"--no-default-features");
        let features = feature_names(&words);
        let host = OnceCell::new();
        let aimed = OnceCell::new();
        let platform = |build: bool| {
            if build {
                host.get_or_init(|| probe(Which::Host)).as_ref()
            } else {
                aimed
                    .get_or_init(|| probe(aim(target_flag(&words), &command_target(&words))))
                    .as_ref()
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
            own.mark(member, dev && (explicit || tests_of(member, doctests)));
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
            built.mark(member, dev && (explicit || tests_of(member, doctests)));
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

fn aim<'a>(flagged: Option<&'a str>, configured: &'a Configured) -> Which<'a> {
    match (flagged, configured) {
        (Some(target), _) => Which::Target(Some(target)),
        (None, Configured::Triple(triple)) => Which::Target(Some(triple)),
        (None, Configured::Unknown) => Which::Unresolved,
        (None, Configured::Absent) => Which::Target(None),
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
