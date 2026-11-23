// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Invocation;

#[derive(Default, Serialize, Deserialize)]
pub(in super::super) struct Selection {
    all: bool,
    #[serde(default)]
    excluded: Vec<String>,
    packages: Vec<String>,
}

impl Selection {
    pub(in super::super) fn selects(&self, package: &str) -> bool {
        !self.excluded.iter().any(|excluded| excluded == package)
            && (self.all || self.packages.iter().any(|selected| selected == package))
    }
}

impl Invocation {
    pub(in super::super) fn selection(&self, members: &BTreeSet<&str>) -> Option<Selection> {
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
        let mut excluded = Vec::new();
        let mut provable = true;
        for (index, word) in words.iter().enumerate() {
            let name = if *word == "--exclude" {
                Some(words.get(index + 1).copied().unwrap_or_default())
            } else {
                word.strip_prefix("--exclude=")
            };
            if let Some(name) = name {
                if members.contains(name) {
                    excluded.push(name.to_owned());
                } else {
                    provable = false;
                }
            }
        }
        let mut packages = Vec::new();
        for (index, word) in words.iter().enumerate() {
            if matches!(*word, "-p" | "--package") {
                match words.get(index + 1) {
                    Some(name) if !name.is_empty() && !name.starts_with('-') => {
                        packages.push((*name).to_owned());
                    }
                    _ => provable = false,
                }
            } else if let Some(name) = word
                .strip_prefix("--package=")
                .or_else(|| word.strip_prefix("-p"))
            {
                if name.is_empty() {
                    provable = false;
                } else {
                    packages.push(name.to_owned());
                }
            }
        }
        let narrowed = words.iter().any(|word| target_filter(word));
        let all = provable
            && !narrowed
            && words
                .iter()
                .any(|word| matches!(*word, "--workspace" | "--all"));
        packages.retain(|name| provable && members.contains(name.as_str()));
        Some(Selection {
            all,
            excluded,
            packages,
        })
    }
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
