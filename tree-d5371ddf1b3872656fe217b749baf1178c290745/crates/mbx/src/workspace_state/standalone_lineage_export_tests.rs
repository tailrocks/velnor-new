//! Native fixture receipts precede capture; no comparison policy supplies ancestry.
use super::*;
use mbx_cache_core::{ActionPrediction, LocalActionCache, RemoteActionResult};
use mbx_cache_store::{ExportForm, ReceiptContext};

struct Chain {
    origin: Fixture,
    destination: Fixture,
    clone: WorkspaceRoots,
    original: ReceiptEvidence,
    current: ReceiptEvidence,
    additions: ExportAdditions,
}

fn capture_records(
    fixture: &Fixture,
    roots: &WorkspaceRoots,
    records: &[ReceiptEvidence],
) -> Result<ExportAdditions> {
    let persisted = publish_receipts(fixture, records)?;
    match crate::workspace_state::capture(
        &fixture.config,
        &fixture.store,
        std::slice::from_ref(roots),
        &fixture.config.cache_dir.join(crate::out_dir::ROOT),
        &persisted,
    )? {
        CaptureOutcome::Captured(additions) | CaptureOutcome::RetainedOwner { additions, .. } => {
            Ok(additions)
        }
        outcome => bail!("native fixture capture explicitly unavailable: {outcome:?}"),
    }
}

fn checked_import(store: &Path, bundle: &Path) -> Result<mbx_cache_store::ImportOutcome> {
    crate::cli::cache::import_cache_archive(store, bundle, None)
}

fn chain(with_action: bool) -> Result<Chain> {
    let origin = fixture()?;
    let destination = fixture()?;
    let mut original = evidence(&origin.original, None).remove(0);
    if with_action {
        let action = CacheDigest::blake3(b"original native action");
        LocalCas::new(&origin.store).store_bytes(&action, b"original native action")?;
        LocalActionCache::new(&origin.store).store(&RemoteActionResult {
            version: 1,
            action: action.clone(),
            metadata: None,
            output_root: None,
        })?;
        original.predictions.push(ActionPrediction {
            action,
            invocation: CacheDigest::blake3(b"original invocation"),
            adapter: "rustc".into(),
            payload: String::from_utf8(mbx_cache_core::canonical_json(
                &mbx_cache_rustc::RustcInputPrediction {
                    version: 4,
                    inputs: vec![],
                    environment: vec![],
                    compiler_duration_ns: 0,
                    crate_name: String::new(),
                },
            )?)?,
        });
    }
    let initial = capture_records(&origin, &origin.original, std::slice::from_ref(&original))?;
    let bundle = origin._directory.path().join("native-a-bundle");
    assert!(
        mbx_cache_store::export_checkout_as(
            &origin.store,
            &origin.original.workspace_root,
            &bundle,
            initial,
            ExportForm::Directory,
        )?
        .exported
    );
    let imported = checked_import(&destination.store, &bundle)?;
    let clone = checkout(destination._directory.path(), "clone-b")?;
    assert!(matches!(
        crate::workspace_state::restore(
            &destination.config,
            &destination.store,
            &imported.attachments[ATTACHMENT],
            &clone.workspace_root,
            &clone.cargo,
        )?,
        RestoreOutcome::Restored { .. }
    ));
    let proof = freeze_lineage(&destination.config, &destination.store, &clone)?
        .ok_or_else(|| eyre::eyre!("native imported lineage missing"))?;
    let current = evidence(&clone, Some(proof)).remove(0);
    let additions = capture_records(&destination, &clone, std::slice::from_ref(&current))?;
    Ok(Chain {
        origin,
        destination,
        clone,
        original,
        current,
        additions,
    })
}

fn receipt_path(store: &Path, record: &ReceiptEvidence) -> Result<PathBuf> {
    let bytes = mbx_cache_core::canonical_json(record)?;
    Ok(store
        .join("build-receipts/v4/evidence")
        .join(format!("{}.json", CacheDigest::blake3(&bytes).hash)))
}

#[cfg(unix)]
#[test]
fn standalone_import_export_import_preserves_exact_original_none_and_owner() -> Result<()> {
    let chain = chain(true)?;
    assert_eq!(chain.original.context, None);
    assert_eq!(chain.original.lineage, None);
    assert_eq!(
        chain.additions.required_receipt_evidence,
        vec![chain.original.clone()]
    );
    let original_owner = state(&chain.destination, &chain.additions)?.workspaces[0]
        .owner
        .clone();
    let expected_action = chain.original.predictions[0].action.clone();
    let bundle = chain
        .destination
        ._directory
        .path()
        .join("standalone-b-bundle");
    // Default native export: no retained ComparisonState and no --compare equivalent.
    assert!(
        mbx_cache_store::export_checkout_as(
            &chain.destination.store,
            &chain.clone.workspace_root,
            &bundle,
            chain.additions,
            ExportForm::Directory,
        )?
        .exported
    );
    let verified = mbx_cache_store::verify_directory_bundle(&bundle)?;
    assert_eq!(verified.actions, 1);
    assert!(
        verified
            .comparison
            .receipt_evidence
            .contains(&chain.original)
    );
    assert!(
        verified
            .comparison
            .receipt_evidence
            .contains(&chain.current)
    );
    let third = fixture()?;
    let imported = checked_import(&third.store, &bundle)?;
    assert!(mbx_cache_store::stored_receipt_evidence(&third.store)?.contains(&chain.original));
    assert_eq!(
        std::fs::read(LocalCas::new(&third.store).path_for(&expected_action)?)?,
        b"original native action"
    );
    let roots = checkout(third._directory.path(), "clone-c")?;
    assert!(matches!(
        crate::workspace_state::restore(
            &third.config,
            &third.store,
            &imported.attachments[ATTACHMENT],
            &roots.workspace_root,
            &roots.cargo,
        )?,
        RestoreOutcome::Restored { .. }
    ));
    let proof = freeze_lineage(&third.config, &third.store, &roots)?
        .ok_or_else(|| eyre::eyre!("third native lineage missing"))?;
    assert_eq!(proof.owner, original_owner);
    assert_eq!(proof.origin, chain.origin.original);
    Ok(())
}

#[cfg(unix)]
#[test]
fn standalone_import_rejects_missing_or_altered_original_before_publication() -> Result<()> {
    for altered in [false, true] {
        let chain = chain(false)?;
        let bundle = chain
            .destination
            ._directory
            .path()
            .join("invalid-original-bundle");
        let attachment = chain.additions.attachments[ATTACHMENT].clone();
        mbx_cache_store::export_checkout_as(
            &chain.destination.store,
            &chain.clone.workspace_root,
            &bundle,
            chain.additions,
            ExportForm::Directory,
        )?;
        let mut records = vec![chain.current.clone()];
        if altered {
            let mut original = chain.original.clone();
            original.context = Some(ReceiptContext {
                schema: 1,
                source: serde_json::json!({"forged": true}),
                tool: serde_json::json!({"forged": true}),
            });
            records.push(original);
        }
        let mut ordered = records
            .into_iter()
            .map(|record| Ok((mbx_cache_core::canonical_json(&record)?, record)))
            .collect::<Result<Vec<_>>>()?;
        ordered.sort_by(|left, right| left.0.cmp(&right.0));
        let records = ordered
            .into_iter()
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        let manifest_path = bundle.join("mbx-cache-export-v5.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
        manifest["receipt_evidence"] = serde_json::to_value(records)?;
        std::fs::write(manifest_path, mbx_cache_core::canonical_json(&manifest)?)?;
        // Generic transport validates; actual CLI owner validation must still refuse.
        mbx_cache_store::verify_directory_bundle(&bundle)?;
        let third = fixture()?;
        let error = checked_import(&third.store, &bundle)
            .expect_err("standalone import must refuse absent or altered original receipts");
        assert!(error.to_string().contains("original receipt closure"));
        assert!(mbx_cache_store::stored_receipt_evidence(&third.store)?.is_empty());
        assert!(LocalCas::new(&third.store).find(&attachment)?.is_none());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn producer_missing_or_altered_original_registry_refuses_capture_and_export() -> Result<()> {
    for altered in [false, true] {
        let chain = chain(false)?;
        let original = receipt_path(&chain.destination.store, &chain.original)?;
        if altered {
            let mut bytes = std::fs::read(&original)?;
            bytes.push(b'\n');
            std::fs::write(&original, bytes)?;
        } else {
            std::fs::remove_file(&original)?;
        }
        assert!(
            capture_records(
                &chain.destination,
                &chain.clone,
                std::slice::from_ref(&chain.current)
            )
            .is_err()
        );
        let bundle = chain.destination._directory.path().join("must-not-publish");
        assert!(
            mbx_cache_store::export_checkout_as(
                &chain.destination.store,
                &chain.clone.workspace_root,
                &bundle,
                chain.additions,
                ExportForm::Directory,
            )
            .is_err()
        );
        assert!(!bundle.exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn standalone_export_excludes_unrelated_persisted_receipts() -> Result<()> {
    let chain = chain(false)?;
    let unrelated = checkout(chain.destination._directory.path(), "unrelated")?;
    let mut records = evidence(&unrelated, None);
    records[0].identity = "c".repeat(64);
    publish_receipts(&chain.destination, &records)?;
    let bundle = chain.destination._directory.path().join("selected-only");
    mbx_cache_store::export_checkout_as(
        &chain.destination.store,
        &chain.clone.workspace_root,
        &bundle,
        chain.additions,
        ExportForm::Directory,
    )?;
    let manifest = mbx_cache_store::verify_directory_bundle(&bundle)?.comparison;
    assert!(manifest.receipt_evidence.contains(&chain.original));
    assert!(manifest.receipt_evidence.contains(&chain.current));
    assert!(!manifest.receipt_evidence.contains(&records[0]));
    assert_eq!(manifest.receipt_evidence.len(), 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn standalone_export_rejects_corrupted_original_action_cas() -> Result<()> {
    let chain = chain(true)?;
    let action = &chain.original.predictions[0].action;
    std::fs::write(
        LocalCas::new(&chain.destination.store).path_for(action)?,
        b"corrupted",
    )?;
    let bundle = chain
        .destination
        ._directory
        .path()
        .join("corrupt-old-action");
    assert!(
        mbx_cache_store::export_checkout_as(
            &chain.destination.store,
            &chain.clone.workspace_root,
            &bundle,
            chain.additions,
            ExportForm::Directory,
        )
        .is_err()
    );
    assert!(!bundle.exists());
    Ok(())
}
