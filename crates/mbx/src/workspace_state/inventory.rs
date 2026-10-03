use super::*;

/// Return stable semantic records carried by a workspace-state attachment.
///
/// The inventory is keyed by a workspace signature, a digest of that
/// workspace's complete semantic entry map, and a normalized relative path.
/// The workspace root and all timestamp metadata stay out of both keys and
/// values.  A workspace digest is the marker, so a deletion changes the
/// marker even when the deleted entry is absent from the after-inventory.  An
/// absent attachment has an empty inventory.
pub(crate) fn semantic_inventory(
    store: &Path,
    attachment: Option<&CacheDigest>,
) -> Result<BTreeMap<String, serde_json::Value>> {
    let Some(attachment) = attachment else {
        return Ok(BTreeMap::new());
    };
    attachment.validate()?;
    let cas = LocalCas::new(store);
    let bundle = lineage::read_bundle(&cas, attachment)?;

    let mut workspaces = bundle
        .workspaces
        .iter()
        .map(|state| semantic_workspace(&cas, state))
        .collect::<Result<Vec<_>>>()?;
    workspaces.sort_by(|left, right| {
        left.signature
            .key()
            .cmp(&right.signature.key())
            .then_with(|| left.digest.key().cmp(&right.digest.key()))
    });

    let mut inventory = BTreeMap::new();
    for workspace in workspaces {
        let prefix = format!(
            "workspace/{}/{}/{}/state/{}/{}/{}",
            workspace.signature.algorithm,
            workspace.signature.hash,
            workspace.signature.size,
            workspace.digest.algorithm,
            workspace.digest.hash,
            workspace.digest.size,
        );
        inventory.insert(
            prefix.clone(),
            serde_json::json!({
                "type": "workspace",
                "content": {
                    "kind": "digest",
                    "digest": workspace.digest,
                },
            }),
        );
        for (path, value) in workspace.entries {
            inventory.insert(format!("{prefix}/{path}"), value);
        }
    }
    validate_semantic_inventory(&inventory)?;
    Ok(inventory)
}

/// Validate the native attachment codec and return precisely its reachable CAS roots.
pub(crate) fn referenced_objects(
    store: &Path,
    attachment: &CacheDigest,
) -> Result<BTreeSet<CacheDigest>> {
    semantic_inventory(store, Some(attachment))?;
    let cas = LocalCas::new(store);
    let bundle = lineage::read_bundle(&cas, attachment)?;
    let mut identities = BTreeSet::new();
    let mut objects = BTreeSet::from([attachment.clone()]);
    for state in bundle.workspaces {
        objects.extend(lineage::owner_objects(&cas, &state.owner)?.1);
        if !identities.insert((
            state.workspace_root,
            state.cargo_roots.target_dir,
            state.cargo_roots.build_dir,
        )) {
            bail!("duplicate workspace root pair");
        }
        for snapshot in state.owned_out_dirs {
            objects.extend(snapshot.files.into_iter().map(|file| file.digest));
        }
        for tree in state.trees {
            objects.insert(tree.inline_archive);
            for reference in tree.references {
                if let FileSource::Cas(digest) = reference.source {
                    objects.insert(digest);
                }
            }
        }
    }
    Ok(objects)
}

/// Match owner view proofs and required generated inputs to native receipt evidence.
pub(crate) fn validate_receipt_evidence(
    store: &Path,
    attachment: &CacheDigest,
    evidence: &[mbx_cache_store::ReceiptEvidence],
) -> Result<()> {
    let cas = LocalCas::new(store);
    let bundle = lineage::read_bundle(&cas, attachment)?;
    for state in bundle.workspaces {
        lineage::validate_original_receipts(&cas, &state.owner, evidence)?;
        let workspace = WorkspaceRoots {
            workspace_root: state.workspace_root,
            cargo: state.cargo_roots,
        };
        crate::out_dir::validate_receipts(&workspace, &state.owned_out_dirs, evidence)?;
    }
    Ok(())
}
