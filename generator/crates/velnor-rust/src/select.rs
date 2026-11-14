// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::classify::{Class, classify};
use crate::config::rule_matches;
use crate::lock_affected::lock_affected;
use crate::manifest_diff::only_dependency_versions_differ;
use crate::roots::CARGO_LOCK;
use velnor_model::{
    AllTrigger, ChangeSet, CompileInputs, MemberDependency, PackageName, RepoFacts, RepoPath,
    RustCiConfig, RustRoot, RustSelection, SelectionReason, Unread,
};

#[must_use]
pub fn select(facts: &RepoFacts, changes: &ChangeSet) -> BTreeMap<RepoPath, RustSelection> {
    let walk = walk(facts, changes);
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
pub fn unknown_paths(facts: &RepoFacts, changes: &ChangeSet) -> BTreeSet<RepoPath> {
    walk(facts, changes).unknown.into_iter().cloned().collect()
}

#[must_use]
pub fn unread_packages(facts: &RepoFacts, changes: &ChangeSet) -> BTreeMap<RepoPath, Unread> {
    let walk = walk(facts, changes);
    walk.unread
        .into_iter()
        .filter(|manifest| walk.reached.contains(manifest))
        .map(|manifest| {
            let unread = facts
                .unowned
                .get(manifest)
                .and_then(|package| package.unread.clone())
                .unwrap_or(Unread::Missing);
            (manifest.clone(), unread)
        })
        .collect()
}

fn walk<'a>(facts: &'a RepoFacts, changes: &'a ChangeSet) -> Walk<'a> {
    let graph = Graph::new(facts, changes);
    let mut walk = Walk {
        triggers: vec![None; facts.rust_roots.len()],
        ..Walk::default()
    };
    for path in &changes.paths {
        graph.place(path, &mut walk);
    }
    while let Some(manifest) = walk.queue.pop() {
        let name = graph.nodes[manifest].name;
        let reason = if graph.unread.contains(manifest) {
            SelectionReason::DependsOnUnread(name.clone())
        } else {
            SelectionReason::DependsOn(name.clone())
        };
        for dependent in graph.dependents.get(manifest).into_iter().flatten() {
            walk.seed(dependent, reason.clone());
        }
        for unread in &graph.unread {
            walk.seed(unread, SelectionReason::DependsOn(name.clone()));
        }
    }
    walk.unread = graph.unread;
    walk
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

pub(crate) fn claims(directory: Option<&RepoPath>, whole: bool, path: &RepoPath) -> bool {
    relative(directory, path).is_some_and(|rest| {
        whole || rest == BUILD_SCRIPT || OWN_FOLDERS.iter().any(|folder| rest.starts_with(folder))
    })
}

struct Graph<'a> {
    roots: &'a [RustRoot],
    changes: &'a ChangeSet,
    config: &'a RustCiConfig,
    inputs: &'a CompileInputs,
    nodes: BTreeMap<&'a RepoPath, Node<'a>>,
    dependents: BTreeMap<&'a RepoPath, Vec<&'a RepoPath>>,
    unread: BTreeSet<&'a RepoPath>,
}

#[derive(Default)]
struct Walk<'a> {
    triggers: Vec<Option<AllTrigger>>,
    reasons: BTreeMap<&'a RepoPath, SelectionReason>,
    reached: BTreeSet<&'a RepoPath>,
    queue: Vec<&'a RepoPath>,
    unknown: BTreeSet<&'a RepoPath>,
    unread: BTreeSet<&'a RepoPath>,
}

impl<'a> Graph<'a> {
    fn new(facts: &'a RepoFacts, changes: &'a ChangeSet) -> Self {
        let roots = &facts.rust_roots;
        let mut graph = Self {
            roots,
            changes,
            config: &facts.config,
            inputs: &facts.compile_inputs,
            nodes: BTreeMap::new(),
            dependents: BTreeMap::new(),
            unread: BTreeSet::new(),
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
                graph.depend(&member.manifest, dependency);
            }
        }
        for (manifest, package) in &facts.unowned {
            graph.nodes.entry(manifest).or_insert(Node {
                root: None,
                name: &package.name,
                directory: manifest.parent(),
                whole: true,
            });
            for dependency in &package.dependencies {
                graph.depend(manifest, dependency);
            }
        }
        graph.unread = graph
            .nodes
            .iter()
            .filter(|(manifest, node)| {
                node.root.is_none()
                    && facts
                        .unowned
                        .get(*manifest)
                        .is_none_or(|package| package.unread.is_some())
            })
            .map(|(manifest, _)| *manifest)
            .collect();
        graph
    }

    fn depend(&mut self, dependent: &'a RepoPath, dependency: &'a MemberDependency) {
        self.dependents
            .entry(&dependency.manifest)
            .or_default()
            .push(dependent);
        self.nodes.entry(&dependency.manifest).or_insert(Node {
            root: None,
            name: &dependency.name,
            directory: dependency.manifest.parent(),
            whole: true,
        });
    }

    fn place(&self, path: &'a RepoPath, walk: &mut Walk<'a>) {
        let text = path.as_str();
        let locked: Vec<&PackageName> = text
            .match_indices('/')
            .map(|(at, _)| &text[..at])
            .chain([text])
            .filter_map(|prefix| self.inputs.get(&Some(RepoPath::new(prefix).ok()?)))
            .flatten()
            .collect();
        self.seed_named(&locked, path, walk);
        let everywhere: Vec<&PackageName> = self.inputs.get(&None).into_iter().flatten().collect();
        self.seed_named(&everywhere, path, walk);
        if self.place_lock(path, walk) {
            return;
        }
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
            .filter(|(_, node)| {
                root.is_some()
                    && node.root == root
                    && claims(node.directory.as_ref(), node.whole, path)
            })
            .max_by_key(|(_, node)| depth(node.directory.as_ref()))
            .map(|(manifest, _)| *manifest);
        if let Some(manifest) = package {
            walk.seed(manifest, SelectionReason::Changed(path.clone()));
            return;
        }
        let is_root_manifest = self.roots.iter().any(|root| root.manifest == *path);
        if is_root_manifest && self.only_dependency_versions_differ(path) {
            return;
        }
        match classify(
            self.config,
            path,
            is_root_manifest,
            self.shelters(path, root),
        ) {
            Class::Nothing => {}
            Class::Packages(names) => self.seed_named(&names, path, walk),
            Class::Global(trigger) => walk.trigger(&trigger),
            Class::Unknown if !locked.is_empty() => {}
            Class::Unknown => {
                walk.unknown.insert(path);
                walk.trigger(&AllTrigger::UnknownPath(path.clone()));
            }
        }
    }

    fn ruled_by_user(&self, path: &RepoPath) -> bool {
        self.config
            .non_inputs
            .iter()
            .any(|glob| rule_matches(glob, path))
            || self
                .config
                .inputs
                .iter()
                .any(|input| rule_matches(&input.glob, path))
    }

    fn place_lock(&self, path: &RepoPath, walk: &mut Walk<'a>) -> bool {
        if self.ruled_by_user(path) {
            return false;
        }
        let Some(root) = self
            .roots
            .iter()
            .find(|root| root.manifest.sibling(&CARGO_LOCK) == *path)
        else {
            return false;
        };
        let names: Vec<PackageName> = root
            .members
            .iter()
            .map(|member| member.name.clone())
            .collect();
        let edited: BTreeSet<PackageName> = root
            .members
            .iter()
            .filter(|member| {
                member.manifest != root.manifest && self.changes.paths.contains(&member.manifest)
            })
            .map(|member| member.name.clone())
            .collect();
        let affected = match (
            self.changes.base_files.get(path),
            self.changes.head_files.get(path),
        ) {
            (Some(base), Some(head)) => lock_affected(base, head, &names, &edited).ok(),
            _ => None,
        };
        match affected {
            Some(affected) => {
                for member in &root.members {
                    if let Some(dependency) = affected.get(&member.name) {
                        walk.seed(&member.manifest, SelectionReason::Lock(dependency.clone()));
                    }
                }
            }
            None => walk.trigger(&AllTrigger::WorkspaceManifest),
        }
        true
    }

    fn only_dependency_versions_differ(&self, manifest: &RepoPath) -> bool {
        let lock = manifest.sibling(&CARGO_LOCK);
        !self.ruled_by_user(manifest)
            && !self.ruled_by_user(&lock)
            && self.changes.paths.contains(&lock)
            && matches!(
                (self.changes.base_files.get(manifest), self.changes.head_files.get(manifest)),
                (Some(base), Some(head)) if only_dependency_versions_differ(base, head)
            )
    }

    fn seed_named(&self, names: &[&PackageName], path: &RepoPath, walk: &mut Walk<'a>) {
        for (manifest, node) in &self.nodes {
            if node.root.is_some() && names.contains(&node.name) {
                walk.seed(manifest, SelectionReason::Input(path.clone()));
            }
        }
    }

    fn shelters(&self, path: &RepoPath, root: Option<usize>) -> bool {
        root.is_some_and(|root| self.roots[root].manifest.parent().is_some())
            || self
                .nodes
                .values()
                .any(|node| node.directory.is_some() && contains(node.directory.as_ref(), path))
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

    fn trigger(&mut self, trigger: &AllTrigger) {
        for slot in &mut self.triggers {
            slot.get_or_insert_with(|| trigger.clone());
        }
    }
}

fn depth(directory: Option<&RepoPath>) -> usize {
    directory.map_or(0, |directory| directory.as_str().len())
}

pub(crate) fn relative<'p>(directory: Option<&RepoPath>, path: &'p RepoPath) -> Option<&'p str> {
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
