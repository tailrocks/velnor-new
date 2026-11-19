// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{CompileInputs, InputOwner, RepoFacts, RustMember, RustRoot};

use crate::error::CiError;

fn directory_of(root: &RustRoot) -> String {
    root.manifest
        .parent()
        .map_or_else(|| ".".to_owned(), |parent| parent.as_str().to_owned())
}

/// Which roots one audit covers: all of them, or with `--root` the root whose
/// manifest sits in that directory plus every root of its workspace group.
pub(super) fn selected(
    facts: &RepoFacts,
    shared: &[Vec<usize>],
    directory: Option<&str>,
) -> Result<Vec<bool>, CiError> {
    let Some(directory) = directory else {
        return Ok(vec![true; facts.rust_roots.len()]);
    };
    let wanted = directory.trim_end_matches('/');
    let wanted = wanted.strip_prefix("./").unwrap_or(wanted);
    let wanted = if wanted.is_empty() { "." } else { wanted };
    let mut chosen: Vec<bool> = facts
        .rust_roots
        .iter()
        .map(|root| directory_of(root) == wanted)
        .collect();
    if !chosen.contains(&true) {
        let known: Vec<String> = facts.rust_roots.iter().map(directory_of).collect();
        return Err(CiError::new(format!(
            "no Cargo root in {wanted}; the roots are {}",
            known.join(", ")
        )));
    }
    for group in shared {
        if group.iter().any(|index| chosen[*index]) {
            for index in group {
                chosen[*index] = true;
            }
        }
    }
    Ok(chosen)
}

/// The lock lines of packages that are members of the selected roots.
pub(super) fn within(listed: CompileInputs, facts: &RepoFacts, chosen: &[bool]) -> CompileInputs {
    let members: Vec<&RustMember> = facts
        .rust_roots
        .iter()
        .zip(chosen)
        .filter(|(_, chosen)| **chosen)
        .flat_map(|(root, _)| &root.members)
        .collect();
    listed
        .into_iter()
        .filter_map(|(path, owners)| {
            let owners: BTreeSet<InputOwner> = owners
                .into_iter()
                .filter(|owner| {
                    members
                        .iter()
                        .any(|member| owner.is(&member.name, &member.manifest))
                })
                .collect();
            (!owners.is_empty()).then_some((path, owners))
        })
        .collect()
}
