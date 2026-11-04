// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::metadata::{WorkspaceRecord, parse_metadata_json};
use crate::metadata_edges::DepKind;
use crate::release_error::ReleaseError;
use crate::release_facts::{DepFact, DepSource, DependencyRegistry};
use crate::release_select::{
    ReleaseRequest, ReleaseSelection, select_release_set, supported_or_default,
};
use semver::{Version, VersionReq};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegistryPackage {
    pub registry: String,
    pub package: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryState {
    pub published: BTreeMap<RegistryPackage, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackagingEdge {
    pub from: String,
    pub to: String,
    pub kind: DepKind,
    pub optional: bool,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub selection: ReleaseSelection,
    pub graph: PublicationGraph,
}

pub fn plan_release(
    request: &ReleaseRequest<'_>,
    registry: &RegistryState,
) -> Result<ReleasePlan, ReleaseError> {
    let selection = select_release_set(request)?;
    if !request.enabled {
        return Ok(ReleasePlan {
            selection,
            graph: PublicationGraph {
                order: Vec::new(),
                edges: Vec::new(),
            },
        });
    }
    let record = parse_metadata_json(
        request.metadata_json,
        request.repo_root,
        request.manifest_hint,
        request.known_manifests,
    )
    .map_err(ReleaseError::Metadata)?;
    let graph = publication_graph(&selection, &record, registry, request.supported_registries)?;
    Ok(ReleasePlan { selection, graph })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationGraph {
    pub order: Vec<String>,
    pub edges: Vec<PackagingEdge>,
}

impl PublicationGraph {
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

struct GraphLookups<'a> {
    selected: BTreeSet<&'a str>,
    by_id: BTreeMap<&'a str, (&'a str, &'a str)>,
    registry: &'a RegistryState,
    supported: Vec<String>,
}

pub fn publication_graph(
    selection: &ReleaseSelection,
    record: &WorkspaceRecord,
    registry: &RegistryState,
    supported: &[String],
) -> Result<PublicationGraph, ReleaseError> {
    if selection.is_empty() {
        return Err(ReleaseError::NothingSelected);
    }
    check_edges_agree(selection, record)?;
    let lookups = GraphLookups {
        selected: selection
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect(),
        by_id: record
            .packages
            .iter()
            .map(|package| {
                (
                    package.id.as_str(),
                    (package.name.as_str(), package.version.as_str()),
                )
            })
            .collect(),
        registry,
        supported: supported_or_default(supported),
    };
    let mut edges = Vec::new();
    for package in &selection.packages {
        let name = package.name.as_str();
        let id = package.id.as_str();
        check_package_deps(name, id, selection, &lookups, &mut edges)?;
    }
    edges.sort();
    let order = topo_order(&lookups.selected, &edges)?;
    Ok(PublicationGraph { order, edges })
}

fn check_edges_agree(
    selection: &ReleaseSelection,
    record: &WorkspaceRecord,
) -> Result<(), ReleaseError> {
    for package in &selection.packages {
        let Some(facts) = selection.facts.deps.get(&package.id) else {
            continue;
        };
        for fact in facts {
            let Some(to) = fact.to.as_deref() else {
                continue;
            };
            let found = record.edges.iter().any(|edge| {
                edge.from == fact.from
                    && edge.to == to
                    && edge.kind == fact.kind
                    && edge.optional == fact.optional
                    && edge.target == fact.target
            });
            if !found {
                return Err(ReleaseError::MetadataMismatch {
                    detail: format!("path edge {} -> {to} disagrees", fact.from),
                });
            }
        }
    }
    Ok(())
}

fn check_package_deps(
    name: &str,
    id: &str,
    selection: &ReleaseSelection,
    lookups: &GraphLookups<'_>,
    edges: &mut Vec<PackagingEdge>,
) -> Result<(), ReleaseError> {
    let Some(facts) = selection.facts.deps.get(id) else {
        return Ok(());
    };
    for fact in facts {
        if fact.kind == DepKind::Dev
            && !fact.version_declared
            && matches!(fact.source, DepSource::Path | DepSource::Git)
        {
            continue;
        }
        check_dep_registry(name, fact, lookups)?;
        let Some(req) = VersionReq::parse(&fact.req).ok() else {
            return Err(ReleaseError::InvalidRequirement {
                package: name.to_owned(),
                dep: fact.name.clone(),
                req: fact.req.clone(),
            });
        };
        match &fact.source {
            DepSource::Git => {
                if !fact.version_declared {
                    return Err(ReleaseError::GitOnlyDep {
                        package: name.to_owned(),
                        dep: fact.name.clone(),
                    });
                }
                check_registry_dep(name, fact, &req, lookups)?;
            }
            DepSource::Other(source) => {
                return Err(ReleaseError::UnsupportedSource {
                    package: name.to_owned(),
                    dep: fact.name.clone(),
                    source: source.clone(),
                });
            }
            DepSource::Registry => check_registry_dep(name, fact, &req, lookups)?,
            DepSource::Path => check_path_dep(name, fact, &req, lookups, edges)?,
        }
    }
    Ok(())
}

fn check_dep_registry(
    name: &str,
    fact: &DepFact,
    lookups: &GraphLookups<'_>,
) -> Result<(), ReleaseError> {
    if let Some(registry) = &fact.registry {
        let supported = match registry {
            DependencyRegistry::Name(name) => lookups.supported.contains(name),
            DependencyRegistry::Index(_) => false,
        };
        if !supported {
            let registry = match registry {
                DependencyRegistry::Name(value) | DependencyRegistry::Index(value) => value,
            };
            return Err(ReleaseError::UnsupportedRegistry {
                package: name.to_owned(),
                registry: registry.clone(),
            });
        }
    }
    Ok(())
}

fn check_registry_dep(
    name: &str,
    fact: &DepFact,
    req: &VersionReq,
    lookups: &GraphLookups<'_>,
) -> Result<(), ReleaseError> {
    if registry_satisfies(lookups.registry, fact.registry.as_ref(), &fact.name, req) {
        Ok(())
    } else {
        Err(ReleaseError::MissingRegistryVersion {
            package: name.to_owned(),
            dep: fact.name.clone(),
            req: fact.req.clone(),
        })
    }
}

fn check_path_dep(
    name: &str,
    fact: &DepFact,
    req: &VersionReq,
    lookups: &GraphLookups<'_>,
    edges: &mut Vec<PackagingEdge>,
) -> Result<(), ReleaseError> {
    if !fact.version_declared {
        return Err(ReleaseError::UnresolvedLocalDep {
            package: name.to_owned(),
            dep: fact.name.clone(),
            path: String::new(),
        });
    }
    let Some(to) = fact.to.as_deref() else {
        return check_registry_dep(name, fact, req, lookups);
    };
    let Some((target, version)) = lookups.by_id.get(to).copied() else {
        return Err(ReleaseError::MetadataMismatch {
            detail: format!("path target {to} is not a reported package"),
        });
    };
    let Some(parsed) = Version::parse(version).ok() else {
        return Err(ReleaseError::InvalidVersion {
            package: target.to_owned(),
            version: version.to_owned(),
        });
    };
    if !req.matches(&parsed) {
        return Err(ReleaseError::RequirementMismatch {
            package: name.to_owned(),
            dep: fact.name.clone(),
            req: fact.req.clone(),
            found: version.to_owned(),
        });
    }
    if !lookups.selected.contains(target) {
        if registry_satisfies(lookups.registry, fact.registry.as_ref(), target, req) {
            return Ok(());
        }
        return Err(ReleaseError::UnpublishedLocalDep {
            dependent: name.to_owned(),
            dep: target.to_owned(),
            req: fact.req.clone(),
        });
    }
    if fact.kind != DepKind::Dev
        || !registry_satisfies(lookups.registry, fact.registry.as_ref(), target, req)
    {
        edges.push(PackagingEdge {
            from: name.to_owned(),
            to: target.to_owned(),
            kind: fact.kind,
            optional: fact.optional,
            target: fact.target.clone(),
        });
    }
    Ok(())
}

fn registry_satisfies(
    state: &RegistryState,
    registry: Option<&DependencyRegistry>,
    name: &str,
    req: &VersionReq,
) -> bool {
    let registry = match registry {
        None => crate::release_select::DEFAULT_REGISTRY,
        Some(DependencyRegistry::Name(name)) => name.as_str(),
        Some(DependencyRegistry::Index(_)) => return false,
    };
    let key = RegistryPackage {
        registry: registry.to_owned(),
        package: name.to_owned(),
    };
    state.published.get(&key).is_some_and(|versions| {
        versions
            .iter()
            .any(|version| Version::parse(version).is_ok_and(|parsed| req.matches(&parsed)))
    })
}

fn topo_order(
    selected: &BTreeSet<&str>,
    edges: &[PackagingEdge],
) -> Result<Vec<String>, ReleaseError> {
    let mut dependents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut pending: BTreeMap<&str, usize> = selected.iter().map(|name| (*name, 0)).collect();
    let mut pairs = BTreeSet::new();
    for edge in edges {
        pairs.insert((edge.to.as_str(), edge.from.as_str()));
    }
    for (dependency, dependent) in pairs {
        dependents.entry(dependency).or_default().insert(dependent);
        if let Some(count) = pending.get_mut(dependent) {
            *count = count.saturating_add(1);
        }
    }
    let mut ready: BTreeSet<&str> = pending
        .iter()
        .filter_map(|(name, count)| if *count == 0 { Some(*name) } else { None })
        .collect();
    let mut order = Vec::with_capacity(selected.len());
    while let Some(next) = ready.pop_first() {
        order.push(next.to_owned());
        if let Some(consumers) = dependents.get(next) {
            for consumer in consumers {
                if let Some(count) = pending.get_mut(consumer) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.insert(consumer);
                    }
                }
            }
        }
    }
    if order.len() == selected.len() {
        Ok(order)
    } else {
        let done: BTreeSet<&str> = order.iter().map(String::as_str).collect();
        let stuck: Vec<String> = selected
            .difference(&done)
            .map(ToString::to_string)
            .collect();
        Err(ReleaseError::PublishCycle { members: stuck })
    }
}
