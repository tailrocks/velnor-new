// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde::{Deserialize, Serialize};

use velnor_model::RustMember;

use super::Invocation;
use super::package_spec::{Spec, parse};

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
        dependencies: &[&str],
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
        let mut packages: Vec<String> = members
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
            .map(|member| member.name.as_str().to_owned())
            .collect();
        if provable && workspace {
            packages.extend(dependencies.iter().map(|name| (*name).to_owned()));
        }
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
    FILTERS.iter().any(|filter| {
        word == *filter
            || word
                .strip_prefix(filter)
                .is_some_and(|rest| rest.starts_with('='))
    })
}
