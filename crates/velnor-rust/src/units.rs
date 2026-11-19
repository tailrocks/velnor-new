// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CacheRoot, RepoFacts, RepoPath, RootSlug, RustRoot};

use crate::base_cache::cache_root;
use crate::roots::CARGO_LOCK;

pub(crate) struct Unit {
    pub(crate) slug: RootSlug,
    pub(crate) gate: RootSlug,
    pub(crate) roots: Vec<RustRoot>,
    pub(crate) grouped: bool,
    pub(crate) target_dir: RepoPath,
}

impl Unit {
    pub(crate) fn single(root: &RustRoot) -> Self {
        Self {
            slug: RootSlug::of(&root.manifest),
            gate: RootSlug::of(&root.manifest),
            roots: vec![root.clone()],
            grouped: false,
            target_dir: root.target_dir.clone(),
        }
    }

    pub(crate) fn directory(&self) -> Option<RepoPath> {
        if self.grouped {
            None
        } else {
            self.roots.first().and_then(|root| root.manifest.parent())
        }
    }

    pub(crate) fn cache_root(&self, facts: &RepoFacts) -> CacheRoot {
        let mut locks: Vec<RepoPath> = self
            .roots
            .iter()
            .map(|root| root.manifest.sibling(&CARGO_LOCK))
            .collect();
        locks.sort();
        match self.roots.first() {
            Some(root) if !self.grouped => cache_root(facts, root),
            _ => CacheRoot::group(
                self.slug.clone(),
                self.slug.to_string(),
                locks,
                self.target_dir.clone(),
            ),
        }
    }
}

pub(crate) fn units(facts: &RepoFacts) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for root in &facts.rust_roots {
        let Some(group) = facts
            .config
            .workspace_groups
            .iter()
            .find(|group| group.roots.contains(&root.manifest))
        else {
            units.push(Unit::single(root));
            continue;
        };
        if !seen.insert(group.name.as_str()) {
            continue;
        }
        let roots = group
            .roots
            .iter()
            .filter_map(|manifest| {
                facts
                    .rust_roots
                    .iter()
                    .find(|root| &root.manifest == manifest)
            })
            .cloned()
            .collect();
        units.push(Unit {
            slug: group.slug.clone(),
            gate: group.gate.clone(),
            roots,
            grouped: true,
            target_dir: group.target_dir(),
        });
    }
    units
}
