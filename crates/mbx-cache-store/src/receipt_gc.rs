//! Receipt authority shares the native cache byte budget.
use super::*;

pub(super) fn entries(store: &Path) -> Result<Vec<Entry>> {
    let mut records = Vec::new();
    for namespace in ["evidence", "checkouts", "groups", "local-grants"] {
        records.extend(walk_files(&store.join(BUILD_RECEIPTS_DIR).join(namespace))?);
    }
    Ok(records)
}

pub(super) fn rooted_paths(store: &Path) -> Result<HashSet<PathBuf>> {
    let mut rooted = HashSet::new();
    let mut selected = Vec::new();
    for namespace in ["checkouts", "groups"] {
        for entry in walk_files(&store.join(BUILD_RECEIPTS_DIR).join(namespace))? {
            if let Some(receipt) = read_build_receipt(store, &entry.path)
                && (namespace == "groups" || latest_claim_is_live(store, &receipt))
            {
                rooted.insert(entry.path);
                selected.push(receipt);
            }
        }
    }
    let evidence = receipt_evidence::selected_evidence(store, &selected)?;
    let mut actions = BTreeSet::new();
    for record in evidence {
        if let Some(lineage) = &record.lineage {
            for digest in &lineage.selected_objects {
                rooted.insert(LocalCas::new(store).path_for(digest)?);
            }
        }
        actions.extend(
            record
                .predictions
                .iter()
                .map(|prediction| prediction.action.clone()),
        );
        let bytes = mbx_cache_core::canonical_json(&record)?;
        rooted.insert(
            store
                .join(BUILD_RECEIPTS_DIR)
                .join("evidence")
                .join(format!("{}.json", CacheDigest::blake3(&bytes).hash)),
        );
    }
    rooted.extend(rooted_action_objects(store, actions));
    Ok(rooted)
}

fn latest_claim_is_live(store: &Path, receipt: &BuildReceipt) -> bool {
    read_checkout_record(&checkout_record_path(
        store,
        &receipt.identity,
        &receipt.workspace_root,
    ))
    .is_some_and(|claim| {
        claim.workspace_root == receipt.workspace_root && claim_is_live(store, &claim)
    })
}

pub(super) fn prune_stale_checkouts(
    store: &Path,
    dry_run: bool,
) -> Result<(HashSet<PathBuf>, u64, u64)> {
    let mut removed = HashSet::new();
    let mut count = 0;
    let mut bytes = 0;
    for entry in walk_files(&store.join(BUILD_RECEIPTS_DIR).join("checkouts"))? {
        if let Some(receipt) = read_build_receipt(store, &entry.path)
            && !checkout_is_live_on(store, &receipt.workspace_root)
        {
            let disposition = if dry_run {
                Removal::Removed
            } else {
                remove(&entry.path)?
            };
            if matches!(disposition, Removal::Removed | Removal::Missing) {
                removed.insert(entry.path);
                if matches!(disposition, Removal::Removed) {
                    count += 1;
                    bytes += entry.size;
                }
            }
        }
    }
    Ok((removed, count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_claim_cannot_re_root_old_receipt_ahead_of_younger_spare() {
        let directory = tempfile::tempdir().unwrap();
        let store = directory.path();
        let workspace = store.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let roots = CargoBuildRoots {
            target_dir: workspace.join("target"),
            build_dir: workspace.join("build"),
        };
        let identity = "a".repeat(64);
        record_checkout(store, &identity, &workspace, Some(&roots)).unwrap();
        let claim_path = checkout_record_path(store, &identity, &workspace);
        let mut claim = read_checkout_record(&claim_path).unwrap();
        claim.updated_secs = 0;
        std::fs::write(claim_path, serde_json::to_vec(&claim).unwrap()).unwrap();
        let cas = LocalCas::new(store);
        let action = CacheDigest::blake3(b"expired original action");
        cas.store_bytes(&action, b"expired original action")
            .unwrap();
        mbx_cache_core::LocalActionCache::new(store)
            .store(&RemoteActionResult {
                version: 1,
                action: action.clone(),
                metadata: None,
                output_root: None,
            })
            .unwrap();
        let context = ReceiptContext {
            schema: 1,
            source: serde_json::json!({"origin":"old"}),
            tool: serde_json::json!({"sdk":"same"}),
        };
        record_build_receipt(
            store,
            &"b".repeat(64),
            &identity,
            &workspace,
            Some(&roots),
            None,
            vec![ActionPrediction {
                action: action.clone(),
                invocation: CacheDigest::blake3(b"invocation"),
                adapter: "rustc".into(),
                payload: "{}".into(),
            }],
            Some(&context),
            None,
        )
        .unwrap();
        let spare = CacheDigest::blake3(b"younger unrelated spare");
        cas.store_bytes(&spare, b"younger unrelated spare").unwrap();
        let old =
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(3600));
        filetime::set_file_times(cas.path_for(&action).unwrap(), old, old).unwrap();
        let outcome = gc(store, stats(store).unwrap().total_bytes() - 1).unwrap();
        assert_eq!(outcome.removed_checkout_records, 1);
        assert!(cas.find(&action).unwrap().is_none());
        assert!(cas.find(&spare).unwrap().is_some());
    }

    #[test]
    fn selected_original_proof_and_action_share_root_priority_after_empty_run() {
        let directory = tempfile::tempdir().unwrap();
        let store = directory.path();
        let workspace = store.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let roots = CargoBuildRoots {
            target_dir: workspace.join("target"),
            build_dir: workspace.join("build"),
        };
        record_checkout(store, &"a".repeat(64), &workspace, Some(&roots)).unwrap();
        let cas = LocalCas::new(store);
        let action = CacheDigest::blake3(b"qualified original action");
        cas.store_bytes(&action, b"qualified original action")
            .unwrap();
        mbx_cache_core::LocalActionCache::new(store)
            .store(&RemoteActionResult {
                version: 1,
                action: action.clone(),
                metadata: None,
                output_root: None,
            })
            .unwrap();
        let original = ReceiptContext {
            schema: 1,
            source: serde_json::json!({"origin":"old"}),
            tool: serde_json::json!({"sdk":"same"}),
        };
        record_build_receipt(
            store,
            &"b".repeat(64),
            &"a".repeat(64),
            &workspace,
            Some(&roots),
            None,
            vec![ActionPrediction {
                action: action.clone(),
                invocation: CacheDigest::blake3(b"invocation"),
                adapter: "rustc".into(),
                payload: "{}".into(),
            }],
            Some(&original),
            None,
        )
        .unwrap();
        let current = ReceiptContext {
            source: serde_json::json!({"origin":"new"}),
            ..original
        };
        record_build_receipt(
            store,
            &"c".repeat(64),
            &"a".repeat(64),
            &workspace,
            Some(&roots),
            None,
            vec![],
            Some(&current),
            None,
        )
        .unwrap();
        let spare = CacheDigest::blake3(b"unrelated spare");
        cas.store_bytes(&spare, b"unrelated spare").unwrap();
        let outcome = gc(store, stats(store).unwrap().total_bytes() - 1).unwrap();
        assert_eq!(outcome.removed_receipt_evidence, 0);
        assert!(cas.find(&action).unwrap().is_some());
        assert!(cas.find(&spare).unwrap().is_none());
        assert_eq!(
            checkout_receipt_evidence(store, &workspace).unwrap().len(),
            2
        );
    }

    #[test]
    fn receipt_only_store_obeys_shared_byte_budget_and_dry_run() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        record_build_receipt(
            directory.path(),
            &"b".repeat(64),
            &"a".repeat(64),
            &workspace,
            Some(&CargoBuildRoots {
                target_dir: workspace.join("target"),
                build_dir: workspace.join("build"),
            }),
            Some("group"),
            Vec::new(),
            Some(&ReceiptContext {
                schema: 1,
                source: serde_json::json!({"fixture":"source"}),
                tool: serde_json::json!({"fixture":"tool"}),
            }),
            None,
        )
        .unwrap();
        let before = stats(directory.path()).unwrap();
        assert_eq!(before.objects, 0);
        assert_eq!(before.receipt_evidence, 3);
        assert!(before.total_bytes() > 0);
        let preview = gc_dry_run(directory.path(), 0).unwrap();
        assert_eq!(preview.removed_receipt_evidence, 3);
        assert_eq!(preview.removed_objects, 0);
        assert_eq!(preview.remaining_bytes, 0);
        assert_eq!(stats(directory.path()).unwrap(), before);
        let actual = gc(directory.path(), 0).unwrap();
        assert_eq!(actual, preview);
        assert_eq!(stats(directory.path()).unwrap().total_bytes(), 0);
        assert!(
            stored_receipt_evidence(directory.path())
                .unwrap()
                .is_empty()
        );
        assert!(
            checkout_receipt_evidence(directory.path(), &workspace)
                .unwrap()
                .is_empty()
        );
    }
}
