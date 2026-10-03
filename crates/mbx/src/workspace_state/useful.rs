//! Conservative replacement coverage over the complete native payload.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplacementCoverage {
    Identical,
    Unavailable(CoverageUnavailableReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CoverageUnavailableReason {
    OwnerIdentityChanged,
    ExecutablePlaceholderNotProven,
    PayloadClosureUnavailable,
    SchedulerValidityNotProven,
}

/// The caller must first validate the frozen owner continuation and exact root
/// role mapping. Physical root scalars belong to that owner proof; every native
/// payload field remains in this comparison, including timestamps, query-cache
/// bytes, generated-view roots and immutable original action/context proofs.
/// Equality proves payload retention only, never compiler-work completeness.
pub(super) fn replacement_coverage(
    cas: &LocalCas,
    retained: &WorkspaceState,
    current: &WorkspaceState,
) -> Result<ReplacementCoverage> {
    validate_state(retained)?;
    validate_state(current)?;
    if retained.owner != current.owner {
        return Ok(ReplacementCoverage::Unavailable(
            CoverageUnavailableReason::OwnerIdentityChanged,
        ));
    }
    if [retained, current].iter().any(|state| {
        state.trees.iter().any(|tree| {
            tree.references
                .iter()
                .any(|reference| matches!(reference.source, FileSource::Mbx))
        })
    }) {
        // Restoration substitutes the current executable. The native
        // placeholder retains no original executable content identity.
        return Ok(ReplacementCoverage::Unavailable(
            CoverageUnavailableReason::ExecutablePlaceholderNotProven,
        ));
    }
    if !complete_closure(cas, retained) || !complete_closure(cas, current) {
        return Ok(ReplacementCoverage::Unavailable(
            CoverageUnavailableReason::PayloadClosureUnavailable,
        ));
    }
    // This checks the native archive codec and role/generated-input inventory.
    // Its projected digest is deliberately not used for payload equivalence.
    semantic_workspace(cas, retained)?;
    semantic_workspace(cas, current)?;
    let payload = |state: &WorkspaceState| {
        mbx_cache_core::canonical_json(&(&state.signature, &state.trees, &state.owned_out_dirs))
    };
    if payload(retained)? == payload(current)? {
        Ok(ReplacementCoverage::Identical)
    } else {
        // Equal action results cannot establish Cargo unit scheduler validity,
        // pruning completeness, or repairs outside mandatory compiler outputs.
        Ok(ReplacementCoverage::Unavailable(
            CoverageUnavailableReason::SchedulerValidityNotProven,
        ))
    }
}

fn complete_closure(cas: &LocalCas, state: &WorkspaceState) -> bool {
    let mut objects = BTreeSet::new();
    for tree in &state.trees {
        objects.insert(&tree.inline_archive);
        for reference in &tree.references {
            if let FileSource::Cas(digest) = &reference.source {
                objects.insert(digest);
            }
        }
    }
    for snapshot in &state.owned_out_dirs {
        objects.extend(snapshot.files.iter().map(|file| &file.digest));
    }
    objects
        .into_iter()
        .all(|digest| cas.find(digest).is_ok_and(|path| path.is_some()))
}

#[cfg(test)]
#[path = "useful_tests.rs"]
mod tests;
