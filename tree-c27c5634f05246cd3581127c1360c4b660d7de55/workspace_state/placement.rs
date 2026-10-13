use super::*;

/// Native cache ownership cannot be atomically republished with Cargo roots.
/// A shared ancestor remains safe when its actual owned namespaces are disjoint.
pub(super) fn overlaps(store: &Path, owner: &Path, roots: &CargoBuildRoots) -> Result<bool> {
    let store = physical_root(&crate::out_dir::resolve_root(store)?)?;
    let mut managed = mbx_cache_store::owned_paths(&store);
    managed.push(crate::out_dir::resolve_root(owner)?);
    let managed = managed
        .iter()
        .map(|path| physical_root(path))
        .collect::<Result<Vec<_>>>()?;
    for role in [&roots.target_dir, &roots.build_dir] {
        // Capture creates temporary archives immediately within the store root.
        if store.starts_with(role) {
            return Ok(true);
        }
        if managed
            .iter()
            .any(|path| path.starts_with(role) || role.starts_with(path))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
