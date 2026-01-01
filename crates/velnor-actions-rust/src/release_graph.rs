//! Packaging-rule publication graph over a release set.
//!
//! Normal, build, optional, and target-specific edges constrain publication
//! order. Dev-edges are dropped per Cargo packaging rules (dev-dependencies
//! never ship in the published manifest), so the conservative CI graph
//! cannot invent publish cycles. Unresolved, Git-only, and registry-missing
//! dependencies fail closed, as does a selected dependent whose local
//! dependency sits outside the authorized set.

use std::collections::{BTreeMap, BTreeSet};

use crate::metadata::WorkspaceRecord;
use crate::metadata_edges::DepKind;
use crate::release_error::ReleaseError;
use crate::release_facts::{DepFact, DepSource};
use crate::release_select::{ReleaseSelection, supported_or_default};
use crate::release_semver::{VersionReq, parse_req, parse_version, req_matches};

/// Observed registry state: package name to published versions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryState {
    /// Published versions per package name.
    pub published: BTreeMap<String, BTreeSet<String>>,
}

/// One packaging edge between selected packages (never a dev-edge).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackagingEdge {
    /// Dependent package name.
    pub from: String,
    /// Dependency package name.
    pub to: String,
    /// Dependency kind (normal or build).
    pub kind: DepKind,
    /// Whether the edge is optional.
    pub optional: bool,
    /// Target filter when target-specific.
    pub target: Option<String>,
}

/// Dependency-ordered publication plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationGraph {
    /// Selected names in dependencies-first order.
    pub order: Vec<String>,
    /// Packaging edges in sorted order.
    pub edges: Vec<PackagingEdge>,
}

impl PublicationGraph {
    /// Number of ordered packages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether the graph holds no packages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Per-package lookups shared across dependency checks.
struct GraphLookups<'a> {
    /// Selected names.
    selected: BTreeSet<&'a str>,
    /// Package id to `(name, version)`.
    by_id: BTreeMap<&'a str, (&'a str, &'a str)>,
    /// Observed registry state.
    registry: &'a RegistryState,
    /// Effective supported registries.
    supported: Vec<String>,
}

/// Build the dependency-ordered publication graph for `selection`.
///
/// # Errors
///
/// Returns [`ReleaseError`] on unparseable requirements, Git-only or
/// unknown sources, missing registry versions, unpublished local
/// dependencies outside the allowlist, metadata disagreement, or cycles.
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

/// Every resolved path edge must exist in the retained metadata graph.
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

/// Check every dependency of one selected package, collecting pack edges.
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
        check_dep_registry(name, fact, lookups)?;
        let Some(req) = parse_req(&fact.req) else {
            return Err(ReleaseError::InvalidRequirement {
                package: name.to_owned(),
                dep: fact.name.clone(),
                req: fact.req.clone(),
            });
        };
        match &fact.source {
            DepSource::Git => {
                return Err(ReleaseError::GitOnlyDep {
                    package: name.to_owned(),
                    dep: fact.name.clone(),
                });
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

/// Reject a dependency restricted to an unsupported registry.
fn check_dep_registry(
    name: &str,
    fact: &DepFact,
    lookups: &GraphLookups<'_>,
) -> Result<(), ReleaseError> {
    if let Some(registry) = fact.registry.as_deref()
        && !lookups
            .supported
            .iter()
            .any(|supported| supported == registry)
    {
        return Err(ReleaseError::UnsupportedRegistry {
            package: name.to_owned(),
            registry: registry.to_owned(),
        });
    }
    Ok(())
}

/// Check a registry dependency against observed published versions.
fn check_registry_dep(
    name: &str,
    fact: &DepFact,
    req: &VersionReq,
    lookups: &GraphLookups<'_>,
) -> Result<(), ReleaseError> {
    if registry_satisfies(lookups.registry, &fact.name, req) {
        Ok(())
    } else {
        Err(ReleaseError::MissingRegistryVersion {
            package: name.to_owned(),
            dep: fact.name.clone(),
            req: fact.req.clone(),
        })
    }
}

/// Check a path dependency: selected targets add edges, others must exist.
fn check_path_dep(
    name: &str,
    fact: &DepFact,
    req: &VersionReq,
    lookups: &GraphLookups<'_>,
    edges: &mut Vec<PackagingEdge>,
) -> Result<(), ReleaseError> {
    let Some(to) = fact.to.as_deref() else {
        return Err(ReleaseError::MetadataMismatch {
            detail: format!("path dep {} of {name} has no target", fact.name),
        });
    };
    let Some((target, version)) = lookups.by_id.get(to).copied() else {
        return Err(ReleaseError::MetadataMismatch {
            detail: format!("path target {to} is not a reported package"),
        });
    };
    let Some(parsed) = parse_version(version) else {
        return Err(ReleaseError::InvalidVersion {
            package: target.to_owned(),
            version: version.to_owned(),
        });
    };
    if !req_matches(req, &parsed) {
        return Err(ReleaseError::RequirementMismatch {
            package: name.to_owned(),
            dep: fact.name.clone(),
            req: fact.req.clone(),
            found: version.to_owned(),
        });
    }
    if !lookups.selected.contains(target) {
        if registry_satisfies(lookups.registry, target, req) {
            return Ok(());
        }
        return Err(ReleaseError::UnpublishedLocalDep {
            dependent: name.to_owned(),
            dep: target.to_owned(),
            req: fact.req.clone(),
        });
    }
    if fact.kind != DepKind::Dev {
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

/// Whether the observed registry holds a version of `name` matching `req`.
fn registry_satisfies(registry: &RegistryState, name: &str, req: &VersionReq) -> bool {
    registry.published.get(name).is_some_and(|versions| {
        versions
            .iter()
            .any(|version| parse_version(version).is_some_and(|parsed| req_matches(req, &parsed)))
    })
}

/// Order selected names dependencies-first (deterministic, lexical).
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
