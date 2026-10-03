use super::*;

pub(super) fn captured_owners(
    config: &Config,
    store: &Path,
    targets: &[WorkspaceRoots],
    evidence: &[mbx_cache_store::ReceiptEvidence],
    mut workspaces: Vec<WorkspaceState>,
) -> Result<(Vec<WorkspaceState>, Vec<String>)> {
    let cas = LocalCas::new(store);
    let mut retained_owner_reasons = Vec::new();
    for (target, state) in targets.iter().zip(&mut workspaces) {
        match lineage::frozen_owner(config, store, target, evidence) {
            Ok(Some(proof)) => {
                state.owner = proof.owner.clone();
                let selected = lineage::read_bundle(&cas, &proof.selected_attachment)?
                    .workspaces
                    .into_iter()
                    .find(|selected| selected.owner == proof.owner)
                    .ok_or_else(|| eyre::eyre!("frozen native owner snapshot is missing"))?;
                match useful::replacement_coverage(&cas, &selected, state)? {
                    useful::ReplacementCoverage::Identical => {}
                    useful::ReplacementCoverage::Unavailable(reason) => {
                        retained_owner_reasons.push(format!(
                            "native owner {} replacement unavailable: {reason:?}",
                            proof.owner.hash
                        ));
                    }
                }
                // Without a changed-view coverage token the original native
                // selected snapshot remains the portable owner snapshot.
                *state = selected;
            }
            Ok(None) => state.owner = lineage::create_owner(&cas, state, evidence)?,
            Err(error) => {
                return Err(error);
            }
        }
    }
    let mut owners = BTreeMap::new();
    for state in workspaces {
        if let Some(previous) = owners.get(&state.owner) {
            if mbx_cache_core::canonical_json(previous)? != mbx_cache_core::canonical_json(&state)?
            {
                bail!("conflicting native captures for the same selected owner");
            }
        } else {
            owners.insert(state.owner.clone(), state);
        }
    }
    let workspaces = owners.into_values().collect::<Vec<_>>();
    Ok((workspaces, retained_owner_reasons))
}

pub(super) fn publish_captures(
    cas: &LocalCas,
    workspaces: Vec<WorkspaceState>,
    retained_owner_reasons: Vec<String>,
) -> Result<CaptureOutcome> {
    let mut objects = BTreeSet::new();
    for state in &workspaces {
        objects.extend(lineage::snapshot_objects(state));
        objects.extend(lineage::owner_objects(cas, &state.owner)?.1);
    }
    if workspaces.is_empty() {
        return Ok(CaptureOutcome::Captured(ExportAdditions::default()));
    }
    let bundle = Bundle {
        version: VERSION,
        workspaces,
    };
    let required_receipt_evidence = lineage::required_receipts(cas, &bundle.workspaces)?;
    validate_bundle(&bundle)?;
    let bytes = mbx_cache_core::canonical_json(&bundle)?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    objects.insert(digest.clone());
    let additions = ExportAdditions {
        attachments: BTreeMap::from([(ATTACHMENT.to_owned(), digest)]),
        objects,
        required_receipt_evidence,
    };
    if retained_owner_reasons.is_empty() {
        Ok(CaptureOutcome::Captured(additions))
    } else {
        Ok(CaptureOutcome::RetainedOwner {
            additions,
            reasons: retained_owner_reasons,
        })
    }
}

pub(super) fn merge_retained(
    cas: &LocalCas,
    states: &mut BTreeMap<CacheDigest, WorkspaceState>,
    current: Bundle,
    unavailable_reasons: &mut Vec<String>,
) -> Result<()> {
    for state in current.workspaces {
        if let Some(retained) = states.get(&state.owner) {
            match useful::replacement_coverage(cas, retained, &state)? {
                useful::ReplacementCoverage::Identical => {
                    // Keep original root provenance; identical payload needs no new view.
                }
                useful::ReplacementCoverage::Unavailable(reason) => {
                    unavailable_reasons.push(format!(
                        "native owner {} replacement unavailable: {reason:?}",
                        state.owner.hash
                    ));
                }
            }
        } else if states.values().any(|retained| {
            retained.signature == state.signature
                || (retained.workspace_root == state.workspace_root
                    && retained.cargo_roots == state.cargo_roots)
        }) {
            // Compatibility cannot distinguish a new origin from a clone
            // whose local continuity proof was lost. It grants no owner.
            unavailable_reasons.push("unbound capture cannot establish distinct ownership relative to baseline; original owner retained".into());
        } else {
            states.insert(state.owner.clone(), state);
        }
    }
    Ok(())
}
