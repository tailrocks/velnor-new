// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    AllTrigger, ChangeSet, PackageName, RepoFacts, RepoPath, RustRoot, RustSelection,
    SelectionReason,
};

#[must_use]
pub fn select(facts: &RepoFacts, changes: &ChangeSet) -> BTreeMap<RepoPath, RustSelection> {
    let graph = Graph::new(&facts.rust_roots);
    let mut walk = Walk {
        triggers: vec![None; facts.rust_roots.len()],
        ..Walk::default()
    };
    for path in &changes.paths {
        graph.place(path, &mut walk);
    }
    while let Some(manifest) = walk.queue.pop() {
        let name = graph.nodes[manifest].name;
        for dependent in graph.dependents.get(manifest).into_iter().flatten() {
            walk.seed(dependent, SelectionReason::DependsOn(name.clone()));
        }
    }
    facts
        .rust_roots
        .iter()
        .zip(walk.triggers)
        .map(|(root, trigger)| {
            let selection = match trigger {
                Some(trigger) => RustSelection::All(trigger),
                None => RustSelection::of(
                    root.members
                        .iter()
                        .filter_map(|member| {
                            let reason = walk.reasons.get(&member.manifest)?;
                            Some((member.name.clone(), reason.clone()))
                        })
                        .collect(),
                    root.members.len(),
                ),
            };
            (root.manifest.clone(), selection)
        })
        .collect()
}

#[must_use]
pub fn root_slug(manifest: &RepoPath) -> String {
    match manifest.parent() {
        None => "root".to_owned(),
        Some(directory) => directory
            .as_str()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect(),
    }
}

struct Node<'a> {
    root: Option<usize>,
    name: &'a PackageName,
    directory: Option<RepoPath>,
}

struct Graph<'a> {
    roots: &'a [RustRoot],
    nodes: BTreeMap<&'a RepoPath, Node<'a>>,
    dependents: BTreeMap<&'a RepoPath, Vec<&'a RepoPath>>,
}

#[derive(Default)]
struct Walk<'a> {
    triggers: Vec<Option<AllTrigger>>,
    reasons: BTreeMap<&'a RepoPath, SelectionReason>,
    reached: BTreeSet<&'a RepoPath>,
    queue: Vec<&'a RepoPath>,
}

impl<'a> Graph<'a> {
    fn new(roots: &'a [RustRoot]) -> Self {
        let mut graph = Self {
            roots,
            nodes: BTreeMap::new(),
            dependents: BTreeMap::new(),
        };
        for (index, root) in roots.iter().enumerate() {
            for member in &root.members {
                graph.nodes.insert(
                    &member.manifest,
                    Node {
                        root: Some(index),
                        name: &member.name,
                        directory: member.directory.clone(),
                    },
                );
            }
        }
        for member in roots.iter().flat_map(|root| &root.members) {
            for dependency in &member.dependencies {
                graph
                    .dependents
                    .entry(&dependency.manifest)
                    .or_default()
                    .push(&member.manifest);
                graph.nodes.entry(&dependency.manifest).or_insert(Node {
                    root: None,
                    name: &dependency.name,
                    directory: dependency.manifest.parent(),
                });
            }
        }
        graph
    }

    fn place(&self, path: &'a RepoPath, walk: &mut Walk<'a>) {
        let root = innermost(
            self.roots
                .iter()
                .enumerate()
                .map(|(index, root)| (index, root.manifest.parent())),
            path,
        );
        let package = innermost(
            self.nodes
                .iter()
                .filter(|(_, node)| root.is_some() && node.root == root)
                .map(|(manifest, node)| (*manifest, node.directory.clone())),
            path,
        );
        match (root, package) {
            (_, Some(manifest)) => walk.seed(manifest, SelectionReason::Changed(path.clone())),
            (Some(index), None) => walk.trigger(&self.roots[index], index, path),
            (None, None) => {
                for (index, root) in self.roots.iter().enumerate() {
                    walk.trigger(root, index, path);
                }
            }
        }
        for (manifest, node) in &self.nodes {
            if node.root.is_none() && contains(node.directory.as_ref(), path) {
                walk.seed(manifest, SelectionReason::Changed(path.clone()));
            }
        }
    }
}

impl<'a> Walk<'a> {
    fn seed(&mut self, manifest: &'a RepoPath, reason: SelectionReason) {
        self.reasons.entry(manifest).or_insert(reason);
        self.reach(manifest);
    }

    fn reach(&mut self, manifest: &'a RepoPath) {
        if self.reached.insert(manifest) {
            self.queue.push(manifest);
        }
    }

    fn trigger(&mut self, root: &'a RustRoot, index: usize, path: &RepoPath) {
        self.triggers[index].get_or_insert_with(|| AllTrigger::UnknownPath(path.clone()));
        for member in &root.members {
            self.reach(&member.manifest);
        }
    }
}

fn innermost<K>(
    candidates: impl Iterator<Item = (K, Option<RepoPath>)>,
    path: &RepoPath,
) -> Option<K> {
    candidates
        .filter(|(_, directory)| contains(directory.as_ref(), path))
        .max_by_key(|(_, directory)| directory.as_ref().map_or(0, |d| d.as_str().len()))
        .map(|(key, _)| key)
}

fn contains(directory: Option<&RepoPath>, path: &RepoPath) -> bool {
    directory.is_none_or(|directory| {
        path.as_str()
            .strip_prefix(directory.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    })
}
