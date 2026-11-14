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

const OWN_FOLDERS: [&str; 4] = ["src/", "tests/", "benches/", "examples/"];
const BUILD_SCRIPT: &str = "build.rs";

struct Node<'a> {
    root: Option<usize>,
    name: &'a PackageName,
    directory: Option<RepoPath>,
    whole: bool,
}

impl Node<'_> {
    fn claims(&self, path: &RepoPath) -> bool {
        relative(self.directory.as_ref(), path).is_some_and(|rest| {
            self.whole
                || rest == BUILD_SCRIPT
                || OWN_FOLDERS.iter().any(|folder| rest.starts_with(folder))
        })
    }
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
                        whole: member.manifest != root.manifest,
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
                    whole: true,
                });
            }
        }
        graph
    }

    fn place(&self, path: &'a RepoPath, walk: &mut Walk<'a>) {
        let root = self
            .roots
            .iter()
            .enumerate()
            .filter(|(_, root)| contains(root.manifest.parent().as_ref(), path))
            .max_by_key(|(_, root)| depth(root.manifest.parent().as_ref()))
            .map(|(index, _)| index);
        let package = self
            .nodes
            .iter()
            .filter(|(_, node)| root.is_some() && node.root == root && node.claims(path))
            .max_by_key(|(_, node)| depth(node.directory.as_ref()))
            .map(|(manifest, _)| *manifest);
        if let Some(manifest) = package {
            walk.seed(manifest, SelectionReason::Changed(path.clone()));
        } else {
            let area = root.and_then(|index| self.roots[index].manifest.parent());
            for (index, candidate) in self.roots.iter().enumerate() {
                if root == Some(index)
                    || within(area.as_ref(), candidate.manifest.parent().as_ref())
                {
                    walk.trigger(candidate, index, path);
                }
            }
        }
        for (manifest, node) in &self.nodes {
            if node.root.is_none() && node.claims(path) {
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

fn depth(directory: Option<&RepoPath>) -> usize {
    directory.map_or(0, |directory| directory.as_str().len())
}

fn within(area: Option<&RepoPath>, directory: Option<&RepoPath>) -> bool {
    directory.is_some_and(|directory| contains(area, directory))
}

fn relative<'p>(directory: Option<&RepoPath>, path: &'p RepoPath) -> Option<&'p str> {
    match directory {
        None => Some(path.as_str()),
        Some(directory) => path
            .as_str()
            .strip_prefix(directory.as_str())?
            .strip_prefix('/'),
    }
}

fn contains(directory: Option<&RepoPath>, path: &RepoPath) -> bool {
    relative(directory, path).is_some()
}
