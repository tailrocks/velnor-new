//! Exact original receipt partitions, with same-tool evidence loss kept explicit.
use super::*;
use mbx_cache_store::{ReceiptContext, ReceiptEvidence};

const JOURNAL: &str = ".owners-v3";

fn workspace_directory(root: &Path, workspace: &WorkspaceRoots) -> Result<PathBuf> {
    if std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("generated output owner journal crosses a symlink");
    }
    source_path(root, &PathBuf::from(JOURNAL).join(journal::key(workspace)?))
}

pub(super) fn directory(
    root: &Path,
    workspace: &WorkspaceRoots,
    context: &ReceiptContext,
) -> Result<PathBuf> {
    context.validate()?;
    let partition = CacheDigest::blake3(&mbx_cache_core::canonical_json(&(
        3_u8, workspace, root, context,
    ))?)
    .hash;
    let parent = workspace_directory(root, workspace)?;
    source_path(&parent, Path::new(&partition))
}

pub(super) fn selected_records(
    root: &Path,
    workspace: &WorkspaceRoots,
    receipts: &[ReceiptEvidence],
    selected: &BTreeMap<evidence::ProofKey, Proof>,
) -> Result<Vec<Snapshot>> {
    let contexts = evidence::contexts(workspace, receipts)?;
    let parent = workspace_directory(root, workspace)?;
    if !parent.try_exists()? {
        return Ok(Vec::new());
    }
    let mut snapshots = Vec::new();
    for partition in std::fs::read_dir(parent)? {
        let partition = partition?;
        if !partition.file_type()?.is_dir() {
            bail!("generated output context partition is not a regular directory");
        }
        for entry in std::fs::read_dir(partition.path())? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "json")
            {
                continue;
            }
            let record = read_record(&entry, workspace, root)?;
            if contexts.is_empty() {
                return Err(
                    OwnerUnavailable("generated output tool evidence is unavailable").into(),
                );
            }
            if !contexts
                .values()
                .any(|context| context.tool == record.context.tool)
            {
                continue;
            }
            if !contexts.contains_key(&mbx_cache_core::canonical_json(&record.context)?) {
                return Err(OwnerUnavailable(
                    "same-tool original generated output receipt is unavailable",
                )
                .into());
            }
            validate_snapshot(&record.snapshot)?;
            for proof in &record.snapshot.proofs {
                if selected.get(&evidence::proof_key(proof)?) != Some(proof) {
                    return Err(OwnerUnavailable(
                        "generated output journal is outside complete selected receipt evidence",
                    )
                    .into());
                }
            }
            snapshots.push(record.snapshot);
        }
    }
    Ok(snapshots)
}

fn read_record(
    entry: &std::fs::DirEntry,
    workspace: &WorkspaceRoots,
    root: &Path,
) -> Result<Record> {
    if !entry.file_type()?.is_file() {
        bail!("generated output owner record is not a regular file");
    }
    let bytes = std::fs::read(entry.path())?;
    let record: Record = serde_json::from_slice(&bytes)?;
    if record.version != 3
        || record.workspace_root != workspace.workspace_root
        || record.cargo_roots != workspace.cargo
    {
        bail!("generated output owner record has a different source binding");
    }
    if record.snapshot.root.as_os_str() != root.as_os_str() {
        return Err(
            OwnerUnavailable("generated output owner root relocation is unavailable").into(),
        );
    }
    record.context.validate()?;
    if mbx_cache_core::canonical_json(&record)? != bytes
        || entry.path().parent() != Some(directory(root, workspace, &record.context)?.as_path())
        || record.snapshot.proofs.is_empty()
        || record
            .snapshot
            .proofs
            .iter()
            .any(|proof| proof.context != record.context)
    {
        bail!("generated output owner record has an invalid receipt partition");
    }
    Ok(record)
}
