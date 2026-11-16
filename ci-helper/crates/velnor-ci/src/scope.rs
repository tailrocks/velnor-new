// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use velnor_model::{RepoFacts, RepoPath};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    prefix: String,
    listed: Vec<String>,
}

impl Scope {
    #[must_use]
    pub fn new(prefix: &str, listed: Vec<String>) -> Self {
        Self {
            prefix: prefix.trim_matches('/').to_owned(),
            listed,
        }
    }

    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        within(path, &self.prefix) || self.listed.iter().any(|listed| within(path, listed))
    }

    pub fn include_packages(&mut self, facts: &RepoFacts, manifest: &str) {
        let mut packages: BTreeMap<_, _> = facts
            .rust_roots
            .iter()
            .flat_map(|root| &root.members)
            .map(|member| (&member.manifest, member.dependencies.as_slice()))
            .collect();
        packages.extend(
            facts
                .unowned
                .iter()
                .map(|(manifest, package)| (manifest, package.dependencies.as_slice())),
        );
        let mut pending: Vec<&RepoPath> = facts
            .rust_roots
            .iter()
            .filter(|root| root.manifest.as_str() == manifest)
            .flat_map(|root| &root.members)
            .map(|member| &member.manifest)
            .collect();
        let mut found = BTreeSet::new();
        while let Some(manifest) = pending.pop() {
            if found.insert(manifest)
                && let Some(dependencies) = packages.get(manifest)
            {
                pending.extend(dependencies.iter().map(|dependency| &dependency.manifest));
            }
        }
        self.listed.extend(found.into_iter().map(|manifest| {
            manifest
                .parent()
                .map_or_else(String::new, |dir| dir.to_string())
        }));
    }
}

fn within(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}
