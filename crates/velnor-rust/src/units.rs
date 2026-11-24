// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{
    CacheRoot, InputsCli, RepoFacts, RepoPath, RootSlug, RustRoot, Step, StepKind, Timings,
};

use crate::base_cache::{cache_root, inputs_cli};
use crate::plan::step;
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

pub(crate) fn restore_file_times(facts: &RepoFacts, units: &[Unit]) -> Vec<Step> {
    let cli = inputs_cli(facts);
    let entries = |unit: &Unit| -> Vec<(RepoPath, RepoPath)> {
        if unit.grouped {
            vec![(RepoPath::from_static("Cargo.toml"), unit.target_dir.clone())]
        } else {
            unit.roots
                .iter()
                .map(|root| (root.manifest.clone(), root.target_dir.clone()))
                .collect()
        }
    };
    if cli == InputsCli::Batched {
        let roots = units.iter().flat_map(entries).collect();
        return vec![step(StepKind::InputsRestore { roots, cli })];
    }
    units
        .iter()
        .flat_map(|unit| {
            if unit.grouped {
                vec![Step {
                    directory: None,
                    ..step(StepKind::InputsRestore {
                        roots: entries(unit),
                        cli,
                    })
                }]
            } else {
                unit.roots
                    .iter()
                    .map(|root| Step {
                        directory: root.manifest.parent(),
                        ..step(StepKind::InputsRestore {
                            roots: vec![(root.manifest.clone(), root.target_dir.clone())],
                            cli,
                        })
                    })
                    .collect()
            }
        })
        .collect()
}

pub(crate) fn timings(facts: &RepoFacts, unit: &Unit) -> Option<Timings> {
    let mut total = Timings::default();
    for root in &unit.roots {
        let timings = facts.timings.get(&root.manifest)?;
        total.lint_build_seconds += timings.lint_build_seconds;
        total.test_build_seconds += timings.test_build_seconds;
        total.test_seconds += timings.test_seconds;
        for (binary, seconds) in &timings.test_binary_seconds {
            *total.test_binary_seconds.entry(binary.clone()).or_default() += seconds;
        }
    }
    Some(total)
}
