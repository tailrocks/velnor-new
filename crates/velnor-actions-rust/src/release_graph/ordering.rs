// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, BTreeSet, DependencyRegistry, PackagingEdge, RegistryPackage, RegistryState,
    ReleaseError, Version, VersionReq,
};

pub(super) fn registry_satisfies(
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

pub(super) fn topo_order(
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
