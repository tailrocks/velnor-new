use super::*;

fn changed_context(original: &Snapshot, source: &str, tool: &str) -> Snapshot {
    let mut snapshot = original.clone();
    let proof = &mut snapshot.proofs[0];
    proof.context.source = serde_json::json!({"source": source});
    proof.context.tool = serde_json::json!({"tool": tool});
    proof.invocation = CacheDigest::blake3(format!("invocation-{source}-{tool}").as_bytes());
    proof.action = CacheDigest::blake3(format!("action-{source}-{tool}").as_bytes());
    snapshot
}

fn capture_selected(
    snapshot: &Snapshot,
    workspace: &WorkspaceRoots,
    receipts: &[mbx_cache_store::ReceiptEvidence],
) -> Result<Vec<Snapshot>> {
    let directory = tempfile::tempdir()?;
    capture(
        &snapshot.root,
        workspace,
        &workspace.cargo.build_dir,
        &mbx_cache_core::LocalCas::new(directory.path()),
        receipts,
    )
}

fn record_files(
    snapshot: &Snapshot,
    workspace: &WorkspaceRoots,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let directory =
        journal::journal_directory(&snapshot.root, workspace, &snapshot.proofs[0].context)?;
    let mut records = std::fs::read_dir(directory)?
        .map(|entry| {
            let path = entry?.path();
            Ok((path.clone(), std::fs::read(path)?))
        })
        .collect::<Result<Vec<_>>>()?;
    records.sort();
    Ok(records)
}

#[test]
fn changed_tool_exports_current_context_and_preserves_old_partition() -> Result<()> {
    let (_directory, workspace, _source, original) = fixture()?;
    let old = changed_context(&original, "source", "A");
    let current = changed_context(&original, "source", "B");
    let mut combined = old.clone();
    combined.proofs.extend(current.proofs.clone());
    register(&old.root, &workspace, &combined)?;
    let old_records = record_files(&old, &workspace)?;
    let current_receipt = owner_receipt(&current, &workspace);
    let captured = capture_selected(&current, &workspace, &[current_receipt.clone()])?;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].proofs, current.proofs);
    assert_eq!(old_records, record_files(&old, &workspace)?);
    let both = capture_selected(
        &current,
        &workspace,
        &[owner_receipt(&old, &workspace), current_receipt],
    )?;
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].proofs.len(), 2);
    assert_eq!(old_records, record_files(&old, &workspace)?);
    release_all_under(&old.root);
    Ok(())
}

#[test]
fn old_tool_partition_cannot_supply_missing_current_owner_proof() -> Result<()> {
    let (_directory, workspace, _source, original) = fixture()?;
    let old = changed_context(&original, "source", "A");
    let current = changed_context(&original, "source", "B");
    register(&old.root, &workspace, &old)?;
    let error = capture_selected(&current, &workspace, &[owner_receipt(&current, &workspace)])
        .expect_err("missing current journal must stay unavailable");
    assert!(is_unavailable(&error));
    release_all_under(&old.root);
    Ok(())
}

#[test]
fn same_tool_original_source_histories_remain_selected() -> Result<()> {
    let (_directory, workspace, _source, original) = fixture()?;
    let old = changed_context(&original, "S1", "tool");
    let current = changed_context(&original, "S2", "tool");
    register(&old.root, &workspace, &old)?;
    register(&current.root, &workspace, &current)?;
    let snapshots = capture_selected(
        &current,
        &workspace,
        &[
            owner_receipt(&old, &workspace),
            owner_receipt(&current, &workspace),
        ],
    )?;
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].proofs.len(), 2);
    assert!(snapshots[0].proofs.contains(&old.proofs[0]));
    assert!(snapshots[0].proofs.contains(&current.proofs[0]));
    release_all_under(&old.root);
    Ok(())
}

#[test]
fn same_tool_missing_source_receipt_cannot_be_inferred_obsolete() -> Result<()> {
    let (_directory, workspace, _source, original) = fixture()?;
    let old = changed_context(&original, "S1", "tool");
    let current = changed_context(&original, "S2", "tool");
    register(&old.root, &workspace, &old)?;
    register(&current.root, &workspace, &current)?;
    let mut current_receipt = owner_receipt(&current, &workspace);
    for empty in [false, true] {
        if empty {
            current_receipt.predictions.clear();
        }
        let error = capture_selected(&current, &workspace, &[current_receipt.clone()])
            .expect_err("same-tool source evidence loss must stay unavailable");
        assert!(is_unavailable(&error));
    }
    release_all_under(&old.root);
    Ok(())
}

#[test]
fn foreign_proof_inside_selected_partition_is_rejected() -> Result<()> {
    let (_directory, workspace, _source, original) = fixture()?;
    let current = changed_context(&original, "source", "B");
    register(&current.root, &workspace, &current)?;
    let records = record_files(&current, &workspace)?;
    let mut record: serde_json::Value = serde_json::from_slice(&records[0].1)?;
    record["snapshot"]["proofs"][0]["invocation"] =
        serde_json::to_value(CacheDigest::blake3(b"foreign-invocation"))?;
    std::fs::write(&records[0].0, mbx_cache_core::canonical_json(&record)?)?;
    let error = capture_selected(&current, &workspace, &[owner_receipt(&current, &workspace)])
        .expect_err("unselected proof inside selected partition must be unavailable");
    assert!(is_unavailable(&error));
    record["snapshot"]["proofs"][0]["context"]["tool"] = serde_json::json!({"tool":"A"});
    std::fs::write(&records[0].0, mbx_cache_core::canonical_json(&record)?)?;
    assert!(
        capture_selected(&current, &workspace, &[owner_receipt(&current, &workspace)]).is_err()
    );
    release_all_under(&current.root);
    Ok(())
}
