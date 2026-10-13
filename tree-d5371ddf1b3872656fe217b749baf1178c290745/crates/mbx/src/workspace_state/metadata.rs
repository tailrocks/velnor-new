use super::*;

pub(super) fn validate_bundle(bundle: &Bundle) -> Result<()> {
    if bundle.version != VERSION || bundle.workspaces.is_empty() {
        bail!("unsupported or invalid Cargo workspace-state attachment");
    }
    let mut identities = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for state in &bundle.workspaces {
        validate_state(state)?;
        if !owners.insert(&state.owner) {
            bail!("duplicate native snapshot owner");
        }
        if !identities.insert((
            &state.workspace_root,
            &state.cargo_roots.target_dir,
            &state.cargo_roots.build_dir,
        )) {
            bail!("duplicate workspace root pair");
        }
    }
    Ok(())
}

pub(super) fn validate_state(state: &WorkspaceState) -> Result<()> {
    state.owner.validate()?;
    state.signature.validate()?;
    for root in [
        &state.workspace_root,
        &state.cargo_roots.target_dir,
        &state.cargo_roots.build_dir,
    ] {
        validate_absolute_root(root)?;
    }
    let expected = if state.cargo_roots.target_dir == state.cargo_roots.build_dir {
        1
    } else {
        2
    };
    if state.trees.len() != expected
        || state.trees.first().map(|tree| tree.role) != Some(RootRole::Target)
        || (expected == 2 && state.trees[1].role != RootRole::Build)
    {
        bail!("invalid workspace root roles");
    }
    for tree in &state.trees {
        validate_tree(tree)?;
    }
    let mut sources = BTreeSet::new();
    for snapshot in &state.owned_out_dirs {
        crate::out_dir::validate_snapshot(snapshot)?;
        if !sources.insert((&snapshot.source, &snapshot.digest)) {
            bail!("duplicate owned OUT_DIR source and digest");
        }
    }
    Ok(())
}

pub(super) fn validate_tree(state: &RootTree) -> Result<()> {
    state.inline_archive.validate()?;
    for metadata in &state.inline_files {
        validate_relative_path(&metadata.path)?;
        if metadata.modified_nanos >= 1_000_000_000 {
            bail!("invalid workspace file timestamp");
        }
    }
    for reference in &state.references {
        validate_relative_path(&reference.path)?;
        if reference.modified_nanos >= 1_000_000_000 {
            bail!("invalid workspace reference timestamp");
        }
        if let FileSource::Cas(digest) = &reference.source {
            digest.validate()?;
        }
    }
    for link in &state.symlinks {
        validate_relative_path(&link.path)?;
        validate_link(&link.path, &link.target)?;
    }
    Ok(())
}

pub(super) fn tree_entries(root: &Path, excluded: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    if !root.try_exists()? {
        return Ok(found);
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = std::fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if excluded.iter().any(|excluded| path.starts_with(excluded)) {
                continue;
            }
            if entry.file_type()?.is_dir() {
                pending.push(path.clone());
            }
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

pub(super) fn workspace_signature(root: &Path) -> Result<CacheDigest> {
    let mut bytes = b"cargo-workspace-state-v4\0".to_vec();
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = root.join(name);
        if path.is_file() {
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(&std::fs::read(path)?);
            bytes.push(0);
        }
    }
    Ok(CacheDigest::blake3(&bytes))
}

pub(super) fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("workspace state contains unsafe path {}", path.display());
    }
    Ok(())
}

pub(super) fn validate_link(path: &Path, target: &Path) -> Result<()> {
    if target.is_absolute() {
        bail!("workspace state contains unsafe link {}", path.display());
    }
    let mut depth = path
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => bail!("workspace state contains unsafe link {}", path.display()),
        }
    }
    Ok(())
}

pub(super) fn modified_parts(metadata: &std::fs::Metadata) -> (u64, u32) {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
        .unwrap_or_default()
}

#[cfg(unix)]
pub(super) fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode()
}

#[cfg(not(unix))]
pub(super) fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

pub(super) fn set_file_metadata(path: &Path, mode: u32, secs: u64, nanos: u32) -> Result<()> {
    if nanos >= 1_000_000_000 {
        bail!("workspace state contains an invalid timestamp");
    }
    let modified = UNIX_EPOCH
        .checked_add(std::time::Duration::new(secs, nanos))
        .ok_or_else(|| eyre::eyre!("workspace state contains an out-of-range timestamp"))?;
    #[cfg(unix)]
    File::options()
        .read(true)
        .open(path)?
        .set_times(FileTimes::new().set_modified(modified))
        .wrap_err_with(|| format!("could not restore the timestamp of {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .wrap_err_with(|| format!("could not restore the mode of {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions.clone())?;
        File::options()
            .write(true)
            .open(path)?
            .set_times(FileTimes::new().set_modified(modified))
            .wrap_err_with(|| format!("could not restore the timestamp of {}", path.display()))?;
        permissions.set_readonly(mode == 1);
        std::fs::set_permissions(path, permissions)
            .wrap_err_with(|| format!("could not restore the permissions of {}", path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn create_symlink(target: &Path, destination: &Path, _directory: bool) -> Result<()> {
    std::os::unix::fs::symlink(target, destination)?;
    Ok(())
}

#[cfg(windows)]
pub(super) fn create_symlink(target: &Path, destination: &Path, directory: bool) -> Result<()> {
    if directory {
        std::os::windows::fs::symlink_dir(target, destination)?;
    } else {
        std::os::windows::fs::symlink_file(target, destination)?;
    }
    Ok(())
}
