//! Successful action ownership for portable generated input views.
use super::*;
pub(super) use partition::directory as journal_directory;

pub(super) fn key(workspace: &WorkspaceRoots) -> Result<String> {
    Ok(CacheDigest::blake3(&serde_json::to_vec(&(
        &workspace.workspace_root,
        &workspace.cargo,
    ))?)
    .hash)
}

pub(crate) fn finalize(invocation: &CacheDigest, action: &CacheDigest) -> Result<()> {
    let Some(real) = ORIGINAL.lock().unwrap().clone().map(PathBuf::from) else {
        return Ok(());
    };
    let workspace = workspace()?;
    let environment = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .ok_or_else(|| eyre::eyre!("generated output owner is missing {name}"))
    };
    let root = environment(ROOT_ENV)?;
    let stable = environment("OUT_DIR")?;
    let source = real.strip_prefix(&workspace.cargo.build_dir)?.to_path_buf();
    let proof = Proof {
        identity: std::env::var(crate::session::BUILD_ENV)?,
        invocation: invocation.clone(),
        action: action.clone(),
        context: context()?,
    };
    let snapshot = snapshot(source, &root, &stable, vec![proof])?;
    register(&root, &workspace, &snapshot)?;
    pending::finish()?;
    Ok(())
}

pub(super) fn workspace() -> Result<WorkspaceRoots> {
    let environment = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .ok_or_else(|| eyre::eyre!("generated output owner is missing {name}"))
    };
    Ok(WorkspaceRoots {
        workspace_root: environment(crate::session::WORKSPACE_ROOT_ENV)?,
        cargo: CargoBuildRoots {
            target_dir: environment(crate::session::TARGET_DIR_ENV)?,
            build_dir: environment(crate::session::BUILD_DIR_ENV)?,
        },
    })
}

pub(crate) fn register(root: &Path, workspace: &WorkspaceRoots, snapshot: &Snapshot) -> Result<()> {
    validate_snapshot(snapshot)?;
    if snapshot.root != root {
        bail!("generated output owner root relocation is unavailable");
    }
    roots::validate_root(root)?;
    validate_existing(&root.join(&snapshot.digest.hash), snapshot)?;
    let mut partitions: BTreeMap<Vec<u8>, Snapshot> = BTreeMap::new();
    for proof in &snapshot.proofs {
        let context = mbx_cache_core::canonical_json(&proof.context)?;
        let partition = partitions.entry(context).or_insert_with(|| {
            let mut partition = snapshot.clone();
            partition.proofs.clear();
            partition
        });
        partition.proofs.push(proof.clone());
    }
    for partition in partitions.into_values() {
        write_record(root, workspace, partition)?;
    }
    Ok(())
}

fn write_record(root: &Path, workspace: &WorkspaceRoots, snapshot: Snapshot) -> Result<()> {
    let context = snapshot.proofs[0].context.clone();
    let directory = journal_directory(root, workspace, &context)?;
    std::fs::create_dir_all(&directory)?;
    let filename = CacheDigest::blake3(&serde_json::to_vec(&(
        &snapshot.source,
        &snapshot.digest,
        &snapshot.proofs,
    ))?)
    .hash;
    let record = Record {
        version: 3,
        workspace_root: workspace.workspace_root.clone(),
        cargo_roots: workspace.cargo.clone(),
        context,
        snapshot,
    };
    let temporary = tempfile::NamedTempFile::new_in(&directory)?;
    std::fs::write(temporary.path(), mbx_cache_core::canonical_json(&record)?)?;
    temporary.persist(directory.join(format!("{filename}.json")))?;
    Ok(())
}

pub(super) fn snapshot(
    source: PathBuf,
    root: &Path,
    stable: &Path,
    proofs: Vec<Proof>,
) -> Result<Snapshot> {
    let (manifest, paths, directories) = describe(stable)?;
    let digest = CacheDigest::blake3(&manifest);
    if stable != root.join(&digest.hash) {
        bail!("generated output does not match its owner identity");
    }
    let files = paths
        .into_iter()
        .map(|(path, executable)| {
            let modified = std::fs::metadata(stable.join(&path))?.modified()?;
            let duration = modified.duration_since(UNIX_EPOCH)?;
            Ok(FileTime {
                digest: CacheDigest::blake3_file(&stable.join(&path))?,
                executable,
                path,
                modified_secs: duration.as_secs(),
                modified_nanos: duration.subsec_nanos(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Snapshot {
        source,
        root: root.to_path_buf(),
        digest,
        directories,
        files,
        proofs,
    })
}

pub(crate) fn capture(
    root: &Path,
    workspace: &WorkspaceRoots,
    physical_build: &Path,
    cas: &mbx_cache_core::LocalCas,
    receipts: &[mbx_cache_store::ReceiptEvidence],
) -> Result<Vec<Snapshot>> {
    roots::validate_root(root)?;
    let (selected, required) = evidence::selection(workspace, receipts)?;
    pending::check(root, workspace)?;
    let mut snapshots = BTreeMap::new();
    let mut supported = BTreeSet::new();
    for snapshot in partition::selected_records(root, workspace, receipts, &selected)? {
        activate_capture(root, physical_build, cas, &snapshot)?;
        cas::store_view(cas, &snapshot)?;
        for proof in &snapshot.proofs {
            supported.insert(evidence::proof_key(proof)?);
        }
        let identity = (snapshot.source.clone(), snapshot.digest.clone());
        if let Some(previous) = snapshots.get_mut(&identity) {
            merge_proofs(previous, snapshot)?;
        } else {
            snapshots.insert(identity, snapshot);
        }
    }
    if !required.is_subset(&supported) {
        return Err(
            OwnerUnavailable("required generated output ownership proof is unavailable").into(),
        );
    }
    Ok(snapshots.into_values().collect())
}

fn activate_capture(
    root: &Path,
    physical_build: &Path,
    cas: &mbx_cache_core::LocalCas,
    snapshot: &Snapshot,
) -> Result<()> {
    let source = source_path(physical_build, &snapshot.source)?;
    if source.try_exists()? && source_matches(&source, snapshot, root)? {
        hydrate(&source, snapshot, root)?;
        return Ok(());
    }
    lease(root, &snapshot.digest.hash)?;
    let stable = root.join(&snapshot.digest.hash);
    if stable.try_exists()? {
        return validate_existing(&stable, snapshot);
    }
    for file in &snapshot.files {
        if cas.find(&file.digest)?.is_none() {
            return Err(OwnerUnavailable("recorded generated output bytes are unavailable").into());
        }
    }
    hydrate_cas(cas, snapshot, root)?;
    Ok(())
}

fn merge_proofs(previous: &mut Snapshot, mut snapshot: Snapshot) -> Result<()> {
    let mut proofs = previous
        .proofs
        .iter()
        .map(|proof| Ok((evidence::proof_key(proof)?, proof.clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    for proof in &snapshot.proofs {
        proofs.insert(evidence::proof_key(proof)?, proof.clone());
    }
    snapshot.proofs = previous.proofs.clone();
    if serde_json::to_vec(previous)? != serde_json::to_vec(&snapshot)? {
        bail!("generated output journals disagree on immutable view metadata");
    }
    previous.proofs = proofs.into_values().collect();
    Ok(())
}

pub(super) fn context() -> Result<mbx_cache_store::ReceiptContext> {
    crate::session::receipt_context_from_environment()?
        .ok_or_else(|| OwnerUnavailable("generated output receipt context is unavailable").into())
}
