use super::*;

/// Capture every recorded Cargo target as a manifest plus an inline metadata tar.
pub(crate) fn capture(store: &Path, targets: &[WorkspaceRoots]) -> Result<ExportAdditions> {
    let executable = std::env::current_exe()?;
    let executable_digest = CacheDigest::blake3_file(&executable)?;
    let cas = LocalCas::new(store);
    let mut objects = BTreeSet::new();
    let mut workspaces = Vec::new();
    for target in targets {
        if !target.workspace_root.is_dir() {
            bail!(
                "recorded Cargo workspace is unavailable: {}",
                target.workspace_root.display()
            );
        }
        workspaces.push(capture_workspace(
            &cas,
            target,
            &executable_digest,
            &mut objects,
        )?);
    }
    if workspaces.is_empty() {
        return Ok(ExportAdditions::default());
    }
    let bytes = serde_json::to_vec(&Bundle {
        version: VERSION,
        workspaces,
    })?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    objects.insert(digest.clone());
    Ok(ExportAdditions {
        attachments: BTreeMap::from([(ATTACHMENT.to_owned(), digest)]),
        objects,
    })
}

/// Retain omitted supported workspaces while current captures replace the same root.
pub(crate) fn retain(
    store: &Path,
    mut additions: ExportAdditions,
    baseline: Option<&CacheDigest>,
) -> Result<ExportAdditions> {
    let Some(baseline) = baseline else {
        return Ok(additions);
    };
    let cas = LocalCas::new(store);
    let load = |digest: &CacheDigest| -> Result<Bundle> {
        let path = cas
            .find(digest)?
            .ok_or_else(|| eyre::eyre!("retained workspace attachment is missing"))?;
        let bundle: Bundle = serde_json::from_slice(&std::fs::read(path)?)?;
        if bundle.version != VERSION || bundle.workspaces.is_empty() {
            bail!("invalid retained workspace attachment");
        }
        for state in &bundle.workspaces {
            semantic_workspace(&cas, state)?;
        }
        Ok(bundle)
    };
    let mut states = load(baseline)?
        .workspaces
        .into_iter()
        .map(|state| {
            (
                (
                    state.workspace_root.clone(),
                    state.cargo_roots.target_dir.clone(),
                    state.cargo_roots.build_dir.clone(),
                ),
                state,
            )
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(current) = additions.attachments.get(ATTACHMENT) {
        for state in load(current)?.workspaces {
            states.insert(
                (
                    state.workspace_root.clone(),
                    state.cargo_roots.target_dir.clone(),
                    state.cargo_roots.build_dir.clone(),
                ),
                state,
            );
        }
    }
    let bundle = Bundle {
        version: VERSION,
        workspaces: states.into_values().collect(),
    };
    for state in &bundle.workspaces {
        for tree in &state.trees {
            additions.objects.insert(tree.inline_archive.clone());
            for reference in &tree.references {
                if let FileSource::Cas(digest) = &reference.source {
                    additions.objects.insert(digest.clone());
                }
            }
        }
    }
    let bytes = serde_json::to_vec(&bundle)?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    additions.objects.insert(digest.clone());
    additions.attachments.insert(ATTACHMENT.to_owned(), digest);
    Ok(additions)
}

pub(super) fn capture_workspace(
    cas: &LocalCas,
    target: &WorkspaceRoots,
    executable_digest: &CacheDigest,
    objects: &mut BTreeSet<CacheDigest>,
) -> Result<WorkspaceState> {
    let (physical, owned_link) = resolve_roots(target)?;
    let mut owned_paths = Vec::new();
    if let Some(link) = owned_link {
        owned_paths.push(link);
        owned_paths.push(physical.target_dir.with_extension("json"));
    }
    let mut trees = vec![capture_tree(
        cas,
        &physical,
        &owned_paths,
        RootRole::Target,
        executable_digest,
        objects,
    )?];
    if target.cargo.build_dir != target.cargo.target_dir {
        trees.push(capture_tree(
            cas,
            &physical,
            &owned_paths,
            RootRole::Build,
            executable_digest,
            objects,
        )?);
    }
    Ok(WorkspaceState {
        workspace_root: target.workspace_root.clone(),
        cargo_roots: target.cargo.clone(),
        signature: workspace_signature(&target.workspace_root)?,
        trees,
    })
}

pub(super) fn capture_tree(
    cas: &LocalCas,
    roots: &CargoBuildRoots,
    owned_paths: &[PathBuf],
    role: RootRole,
    executable_digest: &CacheDigest,
    objects: &mut BTreeSet<CacheDigest>,
) -> Result<RootTree> {
    let root = role_root(roots, role);
    let other = role_root(
        roots,
        if role == RootRole::Target {
            RootRole::Build
        } else {
            RootRole::Target
        },
    );
    let temporary = tempfile::NamedTempFile::new_in(cas.root())?;
    let mut archive = tar::Builder::new(temporary.reopen()?);
    let mut references = Vec::new();
    let mut inline_files = Vec::new();
    let mut symlinks = Vec::new();
    let mut excluded = Vec::new();
    if other != root && other.starts_with(root) {
        excluded.push(other.to_path_buf());
    }
    for path in owned_paths {
        if path != root && path.starts_with(root) {
            excluded.push(path.clone());
        }
    }
    for path in tree_entries(root, &excluded)? {
        let relative = path.strip_prefix(root)?.to_path_buf();
        validate_relative_path(&relative)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        let kind = metadata.file_type();
        if kind.is_dir() {
            archive.append_dir(&relative, &path)?;
        } else if kind.is_symlink() {
            let link = std::fs::read_link(&path)?;
            validate_link(&relative, &link)?;
            symlinks.push(Symlink {
                path: relative,
                target: link,
                directory: path.metadata().is_ok_and(|metadata| metadata.is_dir()),
            });
        } else if kind.is_file() {
            let digest = CacheDigest::blake3_file(&path)?;
            let source = if digest == *executable_digest {
                Some(FileSource::Mbx)
            } else if cas.path_for(&digest)?.is_file() {
                objects.insert(digest.clone());
                Some(FileSource::Cas(digest))
            } else {
                None
            };
            if let Some(source) = source {
                let (modified_secs, modified_nanos) = modified_parts(&metadata);
                references.push(FileReference {
                    path: relative,
                    source,
                    mode: file_mode(&metadata),
                    modified_secs,
                    modified_nanos,
                });
            } else {
                archive.append_path_with_name(&path, &relative)?;
                let (modified_secs, modified_nanos) = modified_parts(&metadata);
                inline_files.push(FileMetadata {
                    path: relative,
                    mode: file_mode(&metadata),
                    modified_secs,
                    modified_nanos,
                });
            }
        } else {
            bail!(
                "Cargo root contains an unsupported entry: {}",
                path.display()
            );
        }
    }
    archive.finish()?;
    drop(archive);
    let inline_archive = CacheDigest::blake3_file(temporary.path())?;
    cas.store_file(&inline_archive, temporary.path())?;
    objects.insert(inline_archive.clone());
    Ok(RootTree {
        role,
        inline_archive,
        inline_files,
        references,
        symlinks,
    })
}

pub(super) fn resolve_roots(target: &WorkspaceRoots) -> Result<(CargoBuildRoots, Option<PathBuf>)> {
    let roots = &target.cargo;
    for path in [&target.workspace_root, &roots.target_dir, &roots.build_dir] {
        validate_absolute_root(path)?;
    }
    let owned = std::fs::symlink_metadata(&roots.target_dir)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    let physical_target = if owned {
        crate::target::owned_capture_root(&target.workspace_root, &roots.target_dir)?
            .ok_or_else(|| eyre::eyre!("Cargo target symlink is not an owned MBX view"))?
    } else {
        physical_root(&roots.target_dir)?
    };
    let physical_build = if roots.build_dir == roots.target_dir {
        physical_target.clone()
    } else if owned && roots.build_dir.starts_with(&roots.target_dir) {
        owned_descendant_root(
            &physical_target,
            roots.build_dir.strip_prefix(&roots.target_dir)?,
        )?
    } else {
        physical_root(&roots.build_dir)?
    };
    if roots.target_dir != roots.build_dir && physical_target == physical_build {
        bail!("Cargo root roles alias the same physical directory");
    }
    if !owned {
        validate_root_relationship(roots, &physical_target, &physical_build)?;
    }
    let owned_link = if owned {
        let parent = roots
            .target_dir
            .parent()
            .ok_or_else(|| eyre::eyre!("Cargo target has no parent"))?;
        let name = roots
            .target_dir
            .file_name()
            .ok_or_else(|| eyre::eyre!("Cargo target has no name"))?;
        Some(parent.canonicalize()?.join(name))
    } else {
        None
    };
    Ok((
        CargoBuildRoots {
            target_dir: physical_target,
            build_dir: physical_build,
        },
        owned_link,
    ))
}

fn owned_descendant_root(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut resolved = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            bail!("invalid owned Cargo root suffix");
        }
        resolved.push(component);
        if std::fs::symlink_metadata(&resolved)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!("Cargo build root crosses an unowned nested symlink");
        }
    }
    physical_root(&resolved)
}
