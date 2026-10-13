//! Match generated views against selected immutable native build receipts.
use super::*;
use mbx_cache_store::ReceiptEvidence;

pub(super) type ProofKey = (String, CacheDigest, CacheDigest, CacheDigest);

pub(super) fn contexts(
    workspace: &WorkspaceRoots,
    evidence: &[ReceiptEvidence],
) -> Result<BTreeMap<Vec<u8>, mbx_cache_store::ReceiptContext>> {
    let mut contexts = BTreeMap::new();
    for receipt in evidence
        .iter()
        .filter(|receipt| receipt.workspace == *workspace)
    {
        if let Some(context) = &receipt.context {
            context.validate()?;
            contexts.insert(mbx_cache_core::canonical_json(context)?, context.clone());
        }
    }
    Ok(contexts)
}

pub(super) fn proof_key(proof: &Proof) -> Result<ProofKey> {
    proof.context.validate()?;
    Ok((
        proof.identity.clone(),
        proof.invocation.clone(),
        proof.action.clone(),
        CacheDigest::blake3(&mbx_cache_core::canonical_json(&proof.context)?),
    ))
}

pub(crate) fn validate_receipts(
    workspace: &WorkspaceRoots,
    snapshots: &[Snapshot],
    evidence: &[ReceiptEvidence],
) -> Result<()> {
    let (selected, required) = selection(workspace, evidence)?;
    let mut supported = BTreeSet::new();
    for snapshot in snapshots {
        validate_snapshot(snapshot)?;
        for proof in &snapshot.proofs {
            let key = proof_key(proof)?;
            if selected.get(&key) != Some(proof) {
                bail!("generated output proof is outside selected immutable receipts");
            }
            supported.insert(key);
        }
    }
    if !required.is_subset(&supported) {
        return Err(
            OwnerUnavailable("required generated output ownership proof is unavailable").into(),
        );
    }
    Ok(())
}

pub(super) fn selection(
    workspace: &WorkspaceRoots,
    evidence: &[ReceiptEvidence],
) -> Result<(BTreeMap<ProofKey, Proof>, BTreeSet<ProofKey>)> {
    let mut selected = BTreeMap::new();
    let mut required = BTreeSet::new();
    let mut matched = false;
    for receipt in evidence
        .iter()
        .filter(|receipt| receipt.workspace == *workspace)
    {
        matched = true;
        for prediction in &receipt.predictions {
            if prediction.adapter != "rustc" {
                continue;
            }
            let parsed: mbx_cache_rustc::RustcInputPrediction =
                serde_json::from_str(&prediction.payload)?;
            if mbx_cache_core::canonical_json(&parsed)? != prediction.payload.as_bytes() {
                bail!("selected generated output prediction is not canonical");
            }
            let Some(context) = &receipt.context else {
                if parsed.requires_owned_out_dir() {
                    return Err(OwnerUnavailable(
                        "required generated output receipt context is unavailable",
                    )
                    .into());
                }
                continue;
            };
            let proof = Proof {
                identity: receipt.identity.clone(),
                invocation: prediction.invocation.clone(),
                action: prediction.action.clone(),
                context: context.clone(),
            };
            if parsed.requires_owned_out_dir() {
                required.insert(proof_key(&proof)?);
            }
            selected.insert(proof_key(&proof)?, proof);
        }
    }
    if !matched {
        return Err(OwnerUnavailable("no selected immutable owner receipt").into());
    }
    Ok((selected, required))
}
