use super::*;

pub(super) fn role_root(roots: &CargoBuildRoots, role: RootRole) -> &Path {
    match role {
        RootRole::Target => &roots.target_dir,
        RootRole::Build => &roots.build_dir,
    }
}

pub(super) fn physical_root(path: &Path) -> Result<PathBuf> {
    validate_absolute_root(path)?;
    let mut existing = path.to_path_buf();
    let mut suffix = Vec::new();
    while !existing.try_exists()? {
        if std::fs::symlink_metadata(&existing)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!(
                "Cargo root resolves through a dangling symlink: {}",
                existing.display()
            );
        }
        suffix.push(
            existing
                .file_name()
                .ok_or_else(|| eyre::eyre!("Cargo root has no existing ancestor"))?
                .to_owned(),
        );
        existing = existing
            .parent()
            .ok_or_else(|| eyre::eyre!("Cargo root has no parent"))?
            .to_path_buf();
    }
    if std::fs::symlink_metadata(&existing)?
        .file_type()
        .is_symlink()
    {
        bail!(
            "Cargo root resolves through a symlink: {}",
            existing.display()
        );
    }
    let mut physical = existing.canonicalize()?;
    for component in suffix.into_iter().rev() {
        physical.push(component);
    }
    Ok(physical)
}

pub(super) fn validate_root_relationship(
    roots: &CargoBuildRoots,
    target: &Path,
    build: &Path,
) -> Result<()> {
    let relation = |left: &Path, right: &Path| -> Option<PathBuf> {
        right.strip_prefix(left).ok().map(Path::to_path_buf)
    };
    if relation(&roots.target_dir, &roots.build_dir) != relation(target, build)
        || relation(&roots.build_dir, &roots.target_dir) != relation(build, target)
    {
        bail!("Cargo roots have aliased or inconsistent physical overlap");
    }
    Ok(())
}

/// Restore every Cargo role only after the entire selected state has staged.
pub(crate) fn restore(
    config: &Config,
    store: &Path,
    attachment: &CacheDigest,
    workspace_root: &Path,
    roots: &CargoBuildRoots,
) -> Result<RestoreOutcome> {
    let logical = WorkspaceRoots {
        workspace_root: workspace_root.to_path_buf(),
        cargo: roots.clone(),
    };
    let active_owner = match lineage::freeze_lineage(config, store, &logical) {
        Ok(proof) => proof.map(|proof| proof.owner),
        Err(error) => {
            log::warn!("previous native restore selector unavailable: {error}");
            None
        }
    };
    lineage::invalidate(config, &logical)?;
    let (destinations, owned_link) = resolve_roots(&logical)?;
    if placement::overlaps(
        store,
        &config.cache_dir.join(crate::out_dir::ROOT),
        &destinations,
    )? {
        return Ok(RestoreOutcome::SkippedManagedOverlap);
    }
    let owned_link = owned_link
        .map(|link| -> Result<OwnedView> {
            let record = destinations.target_dir.with_extension("json");
            Ok(OwnedView {
                target: std::fs::read_link(&link)?,
                link,
                record_bytes: std::fs::read(&record)?,
                record_metadata: std::fs::symlink_metadata(&record)?,
                record,
            })
        })
        .transpose()?;
    let cas = LocalCas::new(store);
    let bundle = lineage::read_bundle(&cas, attachment)?;
    let signature = workspace_signature(workspace_root)?;
    let state = match lineage_select::select_snapshot(
        &bundle,
        workspace_root,
        roots,
        signature,
        active_owner,
    ) {
        Ok(state) => state,
        Err(outcome) => return Ok(outcome),
    };
    let (origin, _) = lineage::owner_objects(&cas, &state.owner)?;
    let current_origin = WorkspaceRoots {
        workspace_root: state.workspace_root.clone(),
        cargo: state.cargo_roots.clone(),
    };
    if origin != current_origin {
        // Owner-preserving changed views require explicit native coverage authority.
        return Ok(RestoreOutcome::SkippedUnavailable);
    }
    let relationship =
        |left: &Path, right: &Path| right.strip_prefix(left).ok().map(Path::to_path_buf);
    if relationship(&state.cargo_roots.target_dir, &state.cargo_roots.build_dir)
        != relationship(&roots.target_dir, &roots.build_dir)
        || relationship(&state.cargo_roots.build_dir, &state.cargo_roots.target_dir)
            != relationship(&roots.build_dir, &roots.target_dir)
    {
        return Ok(RestoreOutcome::SkippedUnavailable);
    }
    if (state.cargo_roots.target_dir == state.cargo_roots.build_dir)
        != (roots.target_dir == roots.build_dir)
    {
        return Ok(RestoreOutcome::SkippedUnavailable);
    }
    // Reject unsafe archive shapes and missing/corrupt objects before touching destinations.
    let semantic = semantic_workspace(&cas, state)?;
    validate_role_entries(&semantic.entries, &state.cargo_roots)?;
    let owner_root = crate::out_dir::resolve_root(&config.cache_dir.join(crate::out_dir::ROOT))?;
    if state
        .owned_out_dirs
        .iter()
        .any(|snapshot| snapshot.root != owner_root)
    {
        return Ok(RestoreOutcome::SkippedUnavailable);
    }
    let original = WorkspaceRoots {
        workspace_root: state.workspace_root.clone(),
        cargo: state.cargo_roots.clone(),
    };
    let evidence = mbx_cache_store::stored_receipt_evidence(store)?;
    if let Err(error) =
        crate::out_dir::validate_receipts(&original, &state.owned_out_dirs, &evidence)
    {
        if crate::out_dir::is_unavailable(&error) {
            return Ok(RestoreOutcome::SkippedUnavailable);
        }
        return Err(error);
    }
    let roots = &destinations;
    validate_role_entries(&semantic.entries, roots)?;
    let mut publications = Vec::new();
    for tree in &state.trees {
        let destination = role_root(roots, tree.role);
        let nested = state.trees.iter().any(|other| {
            other.role != tree.role && destination.starts_with(role_root(roots, other.role))
        });
        if nested {
            continue;
        }
        if !destination_is_empty(destination, roots, owned_link.as_ref())? {
            return Ok(RestoreOutcome::SkippedNonempty);
        }
        let parent = destination
            .parent()
            .ok_or_else(|| eyre::eyre!("Cargo root has no parent"))?;
        std::fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix(".mbx-workspace-state-")
            .tempdir_in(parent)?;
        let staged = staging.path().join("root");
        std::fs::create_dir(&staged)?;
        publications.push((destination.to_path_buf(), staging, staged));
    }
    let executable = std::env::current_exe()?;
    let mut files = 0;
    let mut referenced_bytes = 0;
    for tree in &state.trees {
        let destination = role_root(roots, tree.role);
        let (outer, _, stage) = publications
            .iter()
            .find(|(outer, _, _)| destination.starts_with(outer))
            .ok_or_else(|| eyre::eyre!("Cargo role has no staged root"))?;
        let staged = stage.join(destination.strip_prefix(outer)?);
        std::fs::create_dir_all(&staged)?;
        unpack_inline(&cas, &tree.inline_archive, &staged)?;
        for metadata in &tree.inline_files {
            set_file_metadata(
                &staged.join(&metadata.path),
                metadata.mode,
                metadata.modified_secs,
                metadata.modified_nanos,
            )?;
        }
        referenced_bytes += materialize_references(&cas, &executable, &staged, &tree.references)?;
        files += tree.references.len() as u64;
        restore_symlinks(&staged, &tree.symlinks)?;
    }
    if let Some(owner) = &owned_link {
        for (path, link) in [(&owner.link, true), (&owner.record, false)] {
            if let Some((outer, _, stage)) = publications
                .iter()
                .find(|(outer, _, _)| path.starts_with(outer))
            {
                let staged_path = stage.join(path.strip_prefix(outer)?);
                if let Some(parent) = staged_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                if link {
                    create_symlink(&owner.target, &staged_path, true)?;
                } else {
                    std::fs::write(&staged_path, &owner.record_bytes)?;
                    let (secs, nanos) = modified_parts(&owner.record_metadata);
                    set_file_metadata(
                        &staged_path,
                        file_mode(&owner.record_metadata),
                        secs,
                        nanos,
                    )?;
                }
            }
        }
        if std::fs::read_link(&owner.link)? != owner.target
            || crate::target::owned_capture_root(workspace_root, &owner.link)?.as_deref()
                != Some(roots.target_dir.as_path())
            || !owned_record_unchanged(owner)?
        {
            bail!("owned Cargo target metadata changed before publication");
        }
    }
    owned_out_dirs::hydrate(config, &cas, state, &logical, roots, &publications)?;
    publish_roots(&publications, roots, owned_link.as_ref())?;
    crate::target::touch_managed(config, workspace_root, &roots.target_dir);
    if let Err(error) = lineage::grant_restore(config, store, attachment, state, &logical) {
        // Materialization succeeded; unavailable lineage cannot alter task success.
        log::warn!("native restore lineage unavailable: {error}");
    }
    Ok(RestoreOutcome::Restored {
        files,
        referenced_bytes,
    })
}

pub(super) fn validate_role_entries(
    entries: &BTreeMap<String, serde_json::Value>,
    roots: &CargoBuildRoots,
) -> Result<()> {
    for path in entries.keys() {
        let Some((role, relative)) = path.split_once('/') else {
            continue;
        };
        if role == "out_dir" {
            continue;
        }
        let role = if role == "target" {
            RootRole::Target
        } else {
            RootRole::Build
        };
        let root = role_root(roots, role);
        let other = role_root(
            roots,
            if role == RootRole::Target {
                RootRole::Build
            } else {
                RootRole::Target
            },
        );
        if other != root && other.starts_with(root) && root.join(relative).starts_with(other) {
            bail!("workspace-state entry overlaps another Cargo root role");
        }
    }
    Ok(())
}

pub(super) fn destination_is_empty(
    root: &Path,
    roots: &CargoBuildRoots,
    owned_link: Option<&OwnedView>,
) -> Result<bool> {
    if !root.try_exists()? {
        return Ok(true);
    }
    let nested = [&roots.target_dir, &roots.build_dir]
        .into_iter()
        .find(|other| other.as_path() != root && other.starts_with(root));
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(owner) = owned_link
                && path == owner.link
            {
                if entry.file_type()?.is_symlink() && std::fs::read_link(&path)? == owner.target {
                    continue;
                }
                return Ok(false);
            }
            if let Some(owner) = owned_link
                && path == owner.record
            {
                if owned_record_unchanged(owner)? {
                    continue;
                }
                return Ok(false);
            }
            let declared_ancestor = nested.is_some_and(|nested| nested.starts_with(&path))
                || owned_link.is_some_and(|owner| {
                    owner.link.starts_with(&path) || owner.record.starts_with(&path)
                });
            if !entry.file_type()?.is_dir() || !declared_ancestor {
                return Ok(false);
            }
            pending.push(path);
        }
    }
    Ok(true)
}

pub(super) fn publish_roots(
    publications: &[(PathBuf, tempfile::TempDir, PathBuf)],
    roots: &CargoBuildRoots,
    owned_link: Option<&OwnedView>,
) -> Result<()> {
    let mut changed = Vec::new();
    for (destination, staging, stage) in publications {
        let backup = staging.path().join("original");
        let result = (|| -> Result<()> {
            if destination.try_exists()? {
                if std::fs::symlink_metadata(destination)?
                    .file_type()
                    .is_symlink()
                    || !destination_is_empty(destination, roots, owned_link)?
                {
                    bail!("Cargo root changed before publication");
                }
                std::fs::rename(destination, &backup)?;
            }
            changed.push((destination, stage, backup, false));
            std::fs::rename(stage, destination)?;
            if let Some((_, _, _, published)) = changed.last_mut() {
                *published = true;
            }
            Ok(())
        })();
        if let Err(error) = result {
            for (destination, stage, backup, published) in changed.into_iter().rev() {
                if published {
                    std::fs::rename(destination, stage)?;
                }
                if backup.exists() {
                    std::fs::rename(backup, destination)?;
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

fn owned_record_unchanged(owner: &OwnedView) -> Result<bool> {
    let metadata = std::fs::symlink_metadata(&owner.record)?;
    Ok(metadata.file_type().is_file()
        && file_mode(&metadata) == file_mode(&owner.record_metadata)
        && modified_parts(&metadata) == modified_parts(&owner.record_metadata)
        && std::fs::read(&owner.record)? == owner.record_bytes)
}
