//! Local selected-owner continuity, distinct from external source admission.
use super::*;
use mbx_cache_store::{ReceiptEvidence, ReceiptLineage};

#[path = "lineage_local.rs"]
mod local;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerAnchor {
    version: u8,
    origin: WorkspaceRoots,
    initial_snapshot: CacheDigest,
    original_receipts: Vec<ReceiptEvidence>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InitialSnapshot {
    workspace_root: PathBuf,
    cargo_roots: CargoBuildRoots,
    signature: CacheDigest,
    trees: Vec<RootTree>,
    owned_out_dirs: Vec<crate::out_dir::Snapshot>,
}

fn read_canonical<T: serde::de::DeserializeOwned + Serialize>(
    cas: &LocalCas,
    digest: &CacheDigest,
) -> Result<T> {
    let path = cas
        .find(digest)?
        .ok_or_else(|| eyre::eyre!("native lineage object missing"))?;
    let bytes = std::fs::read(path)?;
    let value: T = serde_json::from_slice(&bytes)?;
    if mbx_cache_core::canonical_json(&value)? != bytes {
        bail!("native lineage object is not canonical");
    }
    Ok(value)
}

fn store_canonical(cas: &LocalCas, value: &impl Serialize) -> Result<CacheDigest> {
    let bytes = mbx_cache_core::canonical_json(value)?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    Ok(digest)
}

pub(super) fn read_bundle(cas: &LocalCas, digest: &CacheDigest) -> Result<Bundle> {
    let bundle = read_canonical(cas, digest)?;
    validate_bundle(&bundle)?;
    for state in &bundle.workspaces {
        verify_owner_state(cas, state)?;
    }
    Ok(bundle)
}

fn initial_payload(state: &WorkspaceState) -> serde_json::Value {
    serde_json::json!({
        "workspace_root": state.workspace_root, "cargo_roots": state.cargo_roots,
        "signature": state.signature, "trees": state.trees,
        "owned_out_dirs": state.owned_out_dirs,
    })
}

fn verify_owner_state(cas: &LocalCas, state: &WorkspaceState) -> Result<()> {
    owner_objects(cas, &state.owner)?;
    let anchor: OwnerAnchor = read_canonical(cas, &state.owner)?;
    let payload = CacheDigest::blake3(&mbx_cache_core::canonical_json(&initial_payload(state))?);
    if payload != anchor.initial_snapshot {
        // Changed native views need a future private completed-coverage token.
        // Canonical JSON and an owner digest cannot authorize reassignment.
        bail!("native snapshot payload does not match its immutable owner anchor");
    }
    Ok(())
}

fn registered_receipts(
    store: &Path,
    roots: &WorkspaceRoots,
    evidence: &[ReceiptEvidence],
) -> Result<Vec<ReceiptEvidence>> {
    let stored = mbx_cache_store::stored_receipt_evidence(store)?
        .iter()
        .map(mbx_cache_core::canonical_json)
        .collect::<Result<BTreeSet<_>>>()?;
    let mut selected = BTreeMap::new();
    for record in evidence.iter().filter(|record| &record.workspace == roots) {
        record.validate()?;
        let canonical = mbx_cache_core::canonical_json(record)?;
        if !stored.contains(&canonical) {
            bail!(
                "selected native snapshot receipt is not persisted in its exact canonical registry"
            );
        }
        selected.insert(canonical, record.clone());
    }
    if selected.is_empty() {
        bail!("native snapshot owner has no original completed receipt evidence");
    }
    Ok(selected.into_values().collect())
}

/// Create one original anchor, never another link in an ancestry chain.
pub(super) fn create_owner(
    cas: &LocalCas,
    state: &WorkspaceState,
    evidence: &[ReceiptEvidence],
) -> Result<CacheDigest> {
    let payload = initial_payload(state);
    let origin = WorkspaceRoots {
        workspace_root: state.workspace_root.clone(),
        cargo: state.cargo_roots.clone(),
    };
    let original_receipts = registered_receipts(cas.root(), &origin, evidence)?;
    let initial_snapshot = store_canonical(cas, &payload)?;
    store_canonical(
        cas,
        &OwnerAnchor {
            version: 1,
            origin,
            initial_snapshot,
            original_receipts,
        },
    )
}

/// Verify the immutable anchor and include its complete original native closure.
pub(super) fn owner_objects(
    cas: &LocalCas,
    owner: &CacheDigest,
) -> Result<(WorkspaceRoots, BTreeSet<CacheDigest>)> {
    let anchor: OwnerAnchor = read_canonical(cas, owner)?;
    if anchor.version != 1 || anchor.original_receipts.is_empty() {
        bail!("invalid native snapshot owner anchor");
    }
    let initial: InitialSnapshot = read_canonical(cas, &anchor.initial_snapshot)?;
    let state = WorkspaceState {
        owner: owner.clone(),
        workspace_root: initial.workspace_root,
        cargo_roots: initial.cargo_roots,
        signature: initial.signature,
        trees: initial.trees,
        owned_out_dirs: initial.owned_out_dirs,
    };
    validate_state(&state)?;
    if state.workspace_root != anchor.origin.workspace_root
        || state.cargo_roots != anchor.origin.cargo
    {
        bail!("native owner origin does not match its initial snapshot");
    }
    let mut encoded = BTreeSet::new();
    let mut ordered = Vec::new();
    for record in &anchor.original_receipts {
        record.validate()?;
        let bytes = mbx_cache_core::canonical_json(record)?;
        ordered.push(bytes.clone());
        if record.workspace != anchor.origin || !encoded.insert(bytes) {
            bail!("native owner original receipts are mismatched or duplicated");
        }
    }
    if ordered != encoded.into_iter().collect::<Vec<_>>() {
        bail!("native owner original receipts are not in canonical order");
    }
    semantic_workspace(cas, &state)?;
    let mut objects = snapshot_objects(&state);
    objects.insert(owner.clone());
    objects.insert(anchor.initial_snapshot);
    Ok((anchor.origin, objects))
}

pub(super) fn owner_receipts(cas: &LocalCas, owner: &CacheDigest) -> Result<Vec<ReceiptEvidence>> {
    owner_objects(cas, owner)?;
    let anchor: OwnerAnchor = read_canonical(cas, owner)?;
    registered_receipts(cas.root(), &anchor.origin, &anchor.original_receipts)
}

/// Imported manifests must preserve the exact anchored originals; local membership is separate.
pub(super) fn validate_original_receipts(
    cas: &LocalCas,
    owner: &CacheDigest,
    evidence: &[ReceiptEvidence],
) -> Result<()> {
    owner_objects(cas, owner)?;
    let anchor: OwnerAnchor = read_canonical(cas, owner)?;
    for original in anchor.original_receipts {
        if !evidence.contains(&original) {
            bail!("native snapshot original receipt closure is absent or altered");
        }
    }
    Ok(())
}

pub(super) fn required_receipts(
    cas: &LocalCas,
    states: &[WorkspaceState],
) -> Result<Vec<ReceiptEvidence>> {
    let mut records = BTreeMap::new();
    for state in states {
        verify_owner_state(cas, state)?;
        for record in owner_receipts(cas, &state.owner)? {
            records.insert(mbx_cache_core::canonical_json(&record)?, record);
        }
    }
    Ok(records.into_values().collect())
}

pub(super) fn snapshot_objects(state: &WorkspaceState) -> BTreeSet<CacheDigest> {
    let mut objects = BTreeSet::new();
    for tree in &state.trees {
        objects.insert(tree.inline_archive.clone());
        for reference in &tree.references {
            if let FileSource::Cas(digest) = &reference.source {
                objects.insert(digest.clone());
            }
        }
    }
    for snapshot in &state.owned_out_dirs {
        objects.extend(snapshot.files.iter().map(|file| file.digest.clone()));
    }
    objects
}

pub(super) fn state_digest(state: &WorkspaceState) -> Result<CacheDigest> {
    Ok(CacheDigest::blake3(&mbx_cache_core::canonical_json(state)?))
}

fn verified_selection(store: &Path, proof: &ReceiptLineage) -> Result<()> {
    proof.validate()?;
    let cas = LocalCas::new(store);
    let bundle = read_bundle(&cas, &proof.selected_attachment)?;
    let states = bundle
        .workspaces
        .iter()
        .filter(|state| state.owner == proof.owner)
        .collect::<Vec<_>>();
    let [state] = states.as_slice() else {
        bail!("native lineage selected owner is not unique");
    };
    if state_digest(state)? != proof.selected_state {
        bail!("native lineage selected snapshot changed");
    }
    let (origin, _) = owner_objects(&cas, &proof.owner)?;
    if origin != proof.origin {
        bail!("native lineage immutable origin changed");
    }
    let objects = referenced_objects(store, &proof.selected_attachment)?;
    if objects.iter().cloned().collect::<Vec<_>>() != proof.selected_objects {
        bail!("native lineage selected closure differs from its exact native object roots");
    }
    for object in objects {
        if cas.find(&object)?.is_none() {
            bail!("native lineage selected closure is incomplete");
        }
    }
    semantic_workspace(&cas, state)?;
    if workspace_signature(&proof.destination.workspace_root)? != state.signature {
        bail!("native lineage workspace compatibility changed");
    }
    Ok(())
}

/// Freeze only a verified local successful-restore capability before a build.
pub(crate) fn freeze_lineage(
    config: &Config,
    store: &Path,
    roots: &WorkspaceRoots,
) -> Result<Option<ReceiptLineage>> {
    let Some(proof) = local::load(config, roots)? else {
        return Ok(None);
    };
    verified_selection(store, &proof)?;
    Ok(Some(proof))
}

/// Revalidate frozen receipt provenance against the live local capability.
pub(super) fn frozen_owner(
    config: &Config,
    store: &Path,
    roots: &WorkspaceRoots,
    evidence: &[ReceiptEvidence],
) -> Result<Option<ReceiptLineage>> {
    let registered = registered_receipts(store, roots, evidence)?;
    let Some(proof) = freeze_lineage(config, store, roots)? else {
        if evidence
            .iter()
            .any(|record| &record.workspace == roots && record.lineage.is_some())
        {
            bail!("frozen native receipt lineage lost its local owner capability");
        }
        return Ok(None);
    };
    let mut matched = false;
    for receipt in &registered {
        if receipt.lineage.as_ref() != Some(&proof) {
            bail!("active native owner lacks matching frozen completed receipt lineage");
        }
        matched = true;
    }
    if !matched {
        bail!("active native owner lacks completed receipt evidence");
    }
    Ok(Some(proof))
}

pub(super) fn invalidate(config: &Config, roots: &WorkspaceRoots) -> Result<()> {
    local::invalidate(config, roots)
}

/// Called only after every destination published; no failed restore grants binding.
pub(super) fn grant_restore(
    config: &Config,
    store: &Path,
    attachment: &CacheDigest,
    state: &WorkspaceState,
    destination: &WorkspaceRoots,
) -> Result<()> {
    let cas = LocalCas::new(store);
    let (origin, _) = owner_objects(&cas, &state.owner)?;
    let physical = physical_roots(destination)?;
    let proof = ReceiptLineage {
        version: 1,
        owner: state.owner.clone(),
        selected_attachment: attachment.clone(),
        selected_state: state_digest(state)?,
        selected_objects: referenced_objects(store, attachment)?.into_iter().collect(),
        origin,
        destination: destination.clone(),
        physical,
    };
    verified_selection(store, &proof)?;
    local::store(config, &proof)
}

fn physical_roots(roots: &WorkspaceRoots) -> Result<WorkspaceRoots> {
    let (cargo, _) = resolve_roots(roots)?;
    Ok(WorkspaceRoots {
        workspace_root: roots.workspace_root.canonicalize()?,
        cargo,
    })
}

#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
