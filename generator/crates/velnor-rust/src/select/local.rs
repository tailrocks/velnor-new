// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{ChangeSet, PackageName, RepoFacts, RepoPath};

use super::{Graph, walk_with_local};

#[must_use]
pub fn affected_local_packages(
    facts: &RepoFacts,
    changes: &ChangeSet,
) -> Option<BTreeMap<RepoPath, PackageName>> {
    if changes.paths.iter().any(|path| {
        path.as_str().rsplit('/').next() == Some("Cargo.toml")
            || crate::classify::global(path, false).is_some()
    }) {
        return None;
    }
    let graph = Graph::new(facts, changes);
    if !graph.unread.is_empty() {
        return None;
    }
    let walk = walk_with_local(facts, changes, true);
    if walk.triggers.iter().any(Option::is_some) || !walk.unknown.is_empty() {
        return None;
    }
    walk.reached
        .into_iter()
        .map(|manifest| Some((manifest.clone(), graph.nodes.get(manifest)?.name.clone())))
        .collect()
}
