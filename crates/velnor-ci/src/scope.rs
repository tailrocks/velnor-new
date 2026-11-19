// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use velnor_model::{CompileInputs, PackageName, RepoFacts, RepoPath};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    prefix: String,
    listed: Vec<String>,
}

impl Scope {
    #[must_use]
    pub fn new(prefix: &str, listed: &CompileInputs, facts: Option<&RepoFacts>) -> Self {
        let mut scope = Self {
            prefix: prefix.trim_matches('/').to_owned(),
            listed: Vec::new(),
        };
        let packages =
            facts.map(|facts| scope.include_packages(facts, &format!("{prefix}Cargo.toml")));
        scope.listed.extend(
            listed
                .iter()
                .filter(|(_, owners)| {
                    packages
                        .as_ref()
                        .is_none_or(|packages| owners.iter().any(|owner| packages.contains(owner)))
                })
                .map(|(path, _)| path.as_ref().map_or_else(String::new, ToString::to_string)),
        );
        scope
    }

    #[must_use]
    pub fn contains_bytes(&self, path: &[u8]) -> bool {
        within(path, self.prefix.as_bytes())
            || self
                .listed
                .iter()
                .any(|listed| within(path, listed.as_bytes()))
    }

    fn include_packages<'a>(
        &mut self,
        facts: &'a RepoFacts,
        manifest: &str,
    ) -> BTreeSet<&'a PackageName> {
        let mut packages: BTreeMap<_, _> = facts
            .rust_roots
            .iter()
            .flat_map(|root| &root.members)
            .map(|member| {
                (
                    &member.manifest,
                    (&member.name, member.dependencies.as_slice()),
                )
            })
            .collect();
        packages.extend(facts.unowned.iter().map(|(manifest, package)| {
            (manifest, (&package.name, package.dependencies.as_slice()))
        }));
        let mut pending: Vec<&RepoPath> = facts
            .rust_roots
            .iter()
            .filter(|root| root.manifest.as_str() == manifest)
            .flat_map(|root| &root.members)
            .map(|member| &member.manifest)
            .collect();
        let mut found = BTreeSet::new();
        let mut names = BTreeSet::new();
        while let Some(manifest) = pending.pop() {
            if found.insert(manifest)
                && let Some((name, dependencies)) = packages.get(manifest)
            {
                names.insert(*name);
                pending.extend(dependencies.iter().map(|dependency| &dependency.manifest));
            }
        }
        self.listed.extend(found.into_iter().map(|manifest| {
            manifest
                .parent()
                .map_or_else(String::new, |dir| dir.to_string())
        }));
        names
    }
}

fn within(path: &[u8], directory: &[u8]) -> bool {
    directory.is_empty()
        || path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with(b"/"))
}
