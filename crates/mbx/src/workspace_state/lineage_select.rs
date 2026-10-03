use super::*;

pub(super) fn select_snapshot<'a>(
    bundle: &'a Bundle,
    workspace_root: &Path,
    roots: &CargoBuildRoots,
    signature: CacheDigest,
    active_owner: Option<CacheDigest>,
) -> std::result::Result<&'a WorkspaceState, RestoreOutcome> {
    let same_pair = bundle
        .workspaces
        .iter()
        .filter(|state| state.workspace_root == workspace_root && state.cargo_roots == *roots)
        .collect::<Vec<_>>();
    let exact = same_pair
        .iter()
        .copied()
        .filter(|state| state.signature == signature)
        .collect::<Vec<_>>();
    let has_active_owner = active_owner.is_some();
    let bound = active_owner
        .map(|owner| {
            bundle
                .workspaces
                .iter()
                .filter(|state| state.owner == owner && state.signature == signature)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if has_active_owner && bound.is_empty() {
        return Err(RestoreOutcome::SkippedUnavailable);
    }
    let matches = if !bound.is_empty() {
        bound
    } else if exact.is_empty() {
        bundle
            .workspaces
            .iter()
            .filter(|state| state.signature == signature)
            .collect::<Vec<_>>()
    } else {
        exact
    };
    if matches.is_empty() && !same_pair.is_empty() {
        return Err(RestoreOutcome::SkippedIncompatible);
    }
    match matches.as_slice() {
        [] => return Err(RestoreOutcome::SkippedUnavailable),
        [state] => Ok(*state),
        _ => return Err(RestoreOutcome::SkippedAmbiguous),
    }
}
