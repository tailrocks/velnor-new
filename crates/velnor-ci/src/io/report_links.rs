// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use velnor_model::{ChangeSet, RepoFacts, RepoPath, RustRoot, RustSelection};

/// Whether a tracked symlink at `path` can feed a compilation of `root`.
///
/// A link counts unless Rust selection, the same rules that decide which members a changed
/// regular file rebuilds, selects nothing for it, and no package or listed compile input
/// lies below it. An unparseable path counts.
pub(super) fn feeds(facts: &RepoFacts, root: &RustRoot, path: &str) -> bool {
    let Ok(path) = RepoPath::new(path) else {
        return true;
    };
    let below = format!("{path}/");
    let under = |directory: Option<&RepoPath>| {
        directory.is_some_and(|directory| directory.as_str().starts_with(&below))
    };
    let packages = facts
        .rust_roots
        .iter()
        .flat_map(|root| &root.members)
        .flat_map(|member| {
            std::iter::once(&member.manifest).chain(
                member
                    .dependencies
                    .iter()
                    .map(|dependency| &dependency.manifest),
            )
        })
        .chain(facts.unowned.keys());
    if packages.into_iter().any(|manifest| under(Some(manifest)))
        || facts
            .compile_inputs
            .keys()
            .any(|input| under(input.as_ref()))
    {
        return true;
    }
    let changes = ChangeSet {
        paths: BTreeSet::from([path]),
        ..ChangeSet::default()
    };
    !matches!(
        velnor_rust::select(facts, &changes).get(&root.manifest),
        Some(RustSelection::None)
    )
}
