// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::{Node, Package, PackageId};

pub(super) enum FeatureContexts {
    Global,
    Scoped(BTreeSet<PackageId>),
}

struct DependencyGraph<'a> {
    dependencies: BTreeMap<&'a PackageId, Vec<&'a PackageId>>,
    uncertain: BTreeSet<PackageId>,
}

impl FeatureContexts {
    pub(super) fn new(packages: &BTreeMap<PackageId, Package>, nodes: &[Node]) -> Self {
        DependencyGraph::new(packages, nodes).map_or(Self::Global, |graph| {
            Self::Scoped(graph.uncertain_packages())
        })
    }

    pub(super) fn uncertain(&self, package: &PackageId) -> bool {
        match self {
            Self::Global => true,
            Self::Scoped(packages) => packages.contains(package),
        }
    }
}

impl<'a> DependencyGraph<'a> {
    fn new(packages: &'a BTreeMap<PackageId, Package>, nodes: &'a [Node]) -> Option<Self> {
        let node_ids: BTreeSet<_> = nodes.iter().map(|node| &node.id).collect();
        let mut graph = Self {
            dependencies: BTreeMap::new(),
            uncertain: packages
                .values()
                .filter(|package| !known_target_context(package))
                .map(|package| package.id.clone())
                .collect(),
        };
        for node in nodes {
            let mut destinations = Vec::new();
            for dependency in node.deps.as_ref()? {
                let destination = dependency.pkg.as_ref()?;
                if !packages.contains_key(destination) || !node_ids.contains(destination) {
                    return None;
                }
                if !dependency.context_free() {
                    graph.uncertain.insert(destination.clone());
                }
                destinations.push(destination);
            }
            graph.dependencies.insert(&node.id, destinations);
        }
        Some(graph)
    }

    fn uncertain_packages(mut self) -> BTreeSet<PackageId> {
        let mut pending: Vec<_> = self.uncertain.iter().cloned().collect();
        while let Some(package) = pending.pop() {
            if let Some(dependencies) = self.dependencies.get(&package) {
                for dependency in dependencies {
                    if self.uncertain.insert((*dependency).clone()) {
                        pending.push((*dependency).clone());
                    }
                }
            }
        }
        self.uncertain
    }
}

fn known_target_context(package: &Package) -> bool {
    package.targets.as_ref().is_some_and(|targets| {
        !targets.is_empty()
            && targets.iter().all(|target| {
                target.kind.as_ref().is_some_and(|kinds| {
                    !kinds.is_empty()
                        && kinds.iter().all(|kind| {
                            matches!(
                                kind.as_str(),
                                "lib"
                                    | "rlib"
                                    | "dylib"
                                    | "cdylib"
                                    | "staticlib"
                                    | "bin"
                                    | "example"
                                    | "test"
                                    | "bench"
                                    | "custom-build"
                            )
                        })
                })
            })
    })
}
