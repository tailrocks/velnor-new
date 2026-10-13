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
    let bundle_path = cas
        .find(attachment)?
        .ok_or_else(|| eyre::eyre!("workspace-state attachment is missing"))?;
    let bundle: Bundle = serde_json::from_slice(&std::fs::read(bundle_path)?)?;
    if bundle.version != VERSION || bundle.workspaces.is_empty() {
        bail!("unsupported or invalid Cargo workspace-state attachment");
    }

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
    Ok(inventory)
}
