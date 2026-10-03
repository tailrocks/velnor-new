//! Native original completed-build authority; context shape is not authentication.
use super::*;

/// Original predictions and context retained before invocation deduplication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEvidence {
    pub workspace: WorkspaceRoots,
    pub identity: String,
    pub context: Option<ReceiptContext>,
    #[serde(deserialize_with = "receipt_lineage::deserialize_required_lineage")]
    pub lineage: Option<ReceiptLineage>,
    pub predictions: Vec<ActionPrediction>,
}

impl ReceiptEvidence {
    pub fn validate(&self) -> Result<()> {
        validate_workspace_paths(&self.workspace.workspace_root, Some(&self.workspace.cargo))?;
        if !is_task_identity(&self.identity) {
            eyre::bail!("invalid receipt evidence identity");
        }
        if let Some(context) = &self.context {
            context.validate()?;
        }
        if let Some(lineage) = &self.lineage {
            lineage.validate()?;
            if lineage.destination != self.workspace {
                eyre::bail!("receipt evidence lineage does not match completed Cargo roots");
            }
        }
        if !(TaskActionManifest {
            version: 1,
            task: self.identity.clone(),
            predictions: self.predictions.clone(),
        })
        .validate()
        {
            eyre::bail!("invalid receipt evidence predictions");
        }
        Ok(())
    }
}

/// Full eligible originals for the checkout selected by native export.
pub fn checkout_receipt_evidence(store: &Path, workspace: &Path) -> Result<Vec<ReceiptEvidence>> {
    let selected = checkout_receipt(store, workspace)
        .into_iter()
        .collect::<Vec<_>>();
    selected_evidence(store, &selected)
}

/// Full eligible originals for all current native grouped export receipts.
pub fn group_receipt_evidence(store: &Path, group: &str) -> Result<Vec<ReceiptEvidence>> {
    let selected = group_receipts(store, group)?
        .into_iter()
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    selected_evidence(store, &selected)
}

/// Read immutable original evidence recorded locally or imported natively.
pub fn stored_receipt_evidence(store: &Path) -> Result<Vec<ReceiptEvidence>> {
    let root = store.join(BUILD_RECEIPTS_DIR).join("evidence");
    let mut records = Vec::new();
    for entry in walk_files(&root)? {
        if entry
            .path
            .parent()
            .is_none_or(|parent| parent.as_os_str() != root.as_os_str())
            || entry.path.extension().and_then(|name| name.to_str()) != Some("json")
            || !entry
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(is_task_identity)
        {
            eyre::bail!("receipt evidence is outside its exact native namespace");
        }
        let bytes = std::fs::read(&entry.path)?;
        let evidence: ReceiptEvidence = serde_json::from_slice(&bytes)?;
        evidence.validate()?;
        let expected = root.join(format!("{}.json", CacheDigest::blake3(&bytes).hash));
        if mbx_cache_core::canonical_json(&evidence)? != bytes
            || entry.path.as_os_str() != expected.as_os_str()
        {
            eyre::bail!("receipt evidence is not native canonical content");
        }
        records.push(evidence);
    }
    canonical_evidence(records)
}

pub(super) fn selected_evidence(
    store: &Path,
    receipts: &[BuildReceipt],
) -> Result<Vec<ReceiptEvidence>> {
    let selected = receipts.iter().filter_map(evidence).collect::<Vec<_>>();
    let mut records = selected.clone();
    for historical in stored_receipt_evidence(store)? {
        if selected.iter().any(|current| {
            current.context.is_some()
                && current.workspace == historical.workspace
                && current.context.as_ref().map(|value| &value.tool)
                    == historical.context.as_ref().map(|value| &value.tool)
        }) {
            records.push(historical);
        }
    }
    canonical_evidence(records)
}

pub(super) fn store_receipt_evidence(store: &Path, receipt: &BuildReceipt) -> Result<()> {
    if let Some(evidence) = evidence(receipt) {
        store_evidence(store, &[evidence])?;
    }
    Ok(())
}

pub(super) fn store_evidence(store: &Path, evidence: &[ReceiptEvidence]) -> Result<()> {
    for record in evidence {
        record.validate()?;
        let bytes = mbx_cache_core::canonical_json(record)?;
        let path = store
            .join(BUILD_RECEIPTS_DIR)
            .join("evidence")
            .join(format!("{}.json", CacheDigest::blake3(&bytes).hash));
        if path.try_exists()? {
            if std::fs::read(&path)? != bytes {
                eyre::bail!("immutable receipt evidence changed");
            }
        } else {
            write_atomic(&path, &bytes)?;
        }
    }
    Ok(())
}

pub(super) fn canonical_evidence(records: Vec<ReceiptEvidence>) -> Result<Vec<ReceiptEvidence>> {
    let mut ordered = BTreeMap::new();
    for record in records {
        record.validate()?;
        ordered.insert(mbx_cache_core::canonical_json(&record)?, record);
    }
    Ok(ordered.into_values().collect())
}

pub(super) fn validate_required_export_evidence(
    store: &Path,
    records: &[ReceiptEvidence],
) -> Result<()> {
    if canonical_evidence(records.to_vec())? != records {
        eyre::bail!("required attachment receipts must be canonical, ordered, and unique");
    }
    let native = stored_receipt_evidence(store)?
        .iter()
        .map(mbx_cache_core::canonical_json)
        .collect::<Result<BTreeSet<_>>>()?;
    for record in records {
        if !native.contains(&mbx_cache_core::canonical_json(record)?) {
            eyre::bail!("required attachment original receipt is not in its exact native registry");
        }
    }
    Ok(())
}

pub(super) fn seed_required_predictions(
    tasks: &mut BTreeMap<String, Vec<ActionPrediction>>,
    records: &[ReceiptEvidence],
) -> Result<()> {
    for record in records {
        let predictions = tasks.entry(record.identity.clone()).or_default();
        for prediction in &record.predictions {
            if let Some(previous) = predictions
                .iter()
                .find(|item| item.invocation == prediction.invocation)
            {
                if previous != prediction {
                    eyre::bail!("required original receipts conflict on a native task invocation");
                }
            } else {
                predictions.push(prediction.clone());
            }
        }
    }
    Ok(())
}

pub(super) fn validate_evidence(
    records: &[ReceiptEvidence],
    owners: &BTreeMap<String, String>,
) -> Result<()> {
    if canonical_evidence(records.to_vec())? != records {
        eyre::bail!("receipt evidence must be canonical, ordered, and unique");
    }
    for record in records {
        for prediction in &record.predictions {
            if owners.get(&serde_json::to_string(&prediction.action)?) != Some(&prediction.adapter)
            {
                eyre::bail!("receipt evidence action does not match its recorded native owner");
            }
        }
    }
    Ok(())
}

pub(super) fn checkout_receipt(store: &Path, workspace: &Path) -> Option<BuildReceipt> {
    read_build_receipt(store, &latest_receipt_path(store, workspace))
        .filter(|value| value.workspace_root == workspace)
}

pub(super) fn group_receipts(store: &Path, group: &str) -> Result<Vec<(PathBuf, BuildReceipt)>> {
    validate_export_group(group)?;
    let root = store
        .join(BUILD_RECEIPTS_DIR)
        .join("groups")
        .join(group_key(group));
    Ok(walk_files(&root)?
        .into_iter()
        .filter_map(|entry| {
            read_build_receipt(store, &entry.path).map(|receipt| (entry.path, receipt))
        })
        .filter(|(_, receipt)| receipt.group.as_deref() == Some(group))
        .collect())
}

fn evidence(receipt: &BuildReceipt) -> Option<ReceiptEvidence> {
    Some(ReceiptEvidence {
        workspace: workspace_roots_for_receipt(receipt)?,
        identity: receipt.identity.clone(),
        context: receipt.context.clone(),
        lineage: receipt.lineage.clone(),
        predictions: receipt.predictions.clone(),
    })
}

pub(super) fn validate_workspace_paths(
    workspace: &Path,
    cargo: Option<&CargoBuildRoots>,
) -> Result<()> {
    for path in [
        Some(workspace),
        cargo.map(|roots| roots.target_dir.as_path()),
        cargo.map(|roots| roots.build_dir.as_path()),
    ]
    .into_iter()
    .flatten()
    {
        if path.to_str().is_none() {
            eyre::bail!("receipt roots must be UTF-8");
        }
        if !path.is_absolute()
            || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            eyre::bail!("receipt roots must be canonical absolute UTF-8 paths");
        }
    }
    Ok(())
}

pub(super) fn receipt_path_matches(store: &Path, path: &Path, receipt: &BuildReceipt) -> bool {
    path.as_os_str() == latest_receipt_path(store, &receipt.workspace_root).as_os_str()
        || receipt.group.as_ref().is_some_and(|group| {
            path.as_os_str() == group_receipt_path(store, group, &receipt.run).as_os_str()
        })
}

pub(super) fn is_receipt_path(store: &Path, path: &Path) -> bool {
    let root = store.join(BUILD_RECEIPTS_DIR);
    let Ok(relative) = path.strip_prefix(&root) else {
        return false;
    };
    if path.extension().and_then(|name| name.to_str()) != Some("json")
        || !path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(is_task_identity)
    {
        return false;
    }
    let components = relative.components().collect::<Vec<_>>();
    match components.as_slice() {
        [Component::Normal(namespace), Component::Normal(_)] => *namespace == "checkouts",
        [
            Component::Normal(namespace),
            Component::Normal(group),
            Component::Normal(_),
        ] => *namespace == "groups" && group.to_str().is_some_and(is_task_identity),
        _ => false,
    }
}

/// Portable owner projection: original contexts, root relationships, and all predictions.
/// Physical checkout paths remain available separately for native proof matching.
pub fn semantic_receipt_evidence(records: &[ReceiptEvidence]) -> Result<Vec<serde_json::Value>> {
    let mut projected = BTreeMap::new();
    for record in records {
        record.validate()?;
        let roots = &record.workspace;
        let location = |path: &Path, base: &Path, role: &str| {
            path.strip_prefix(base)
                .ok()
                .map(|relative| serde_json::json!([role, relative]))
        };
        let target = location(&roots.cargo.target_dir, &roots.workspace_root, "workspace")
            .unwrap_or_else(|| serde_json::json!(["target", ""]));
        let build = location(&roots.cargo.build_dir, &roots.cargo.target_dir, "target")
            .or_else(|| location(&roots.cargo.build_dir, &roots.workspace_root, "workspace"))
            .unwrap_or_else(|| serde_json::json!(["build", ""]));
        let value = serde_json::json!({"identity":record.identity,"context":record.context,
            "target":target,"build":build,"predictions":record.predictions});
        projected.insert(mbx_cache_core::canonical_json(&value)?, value);
    }
    Ok(projected.into_values().collect())
}

#[cfg(test)]
#[path = "receipt_evidence_tests.rs"]
mod tests;
