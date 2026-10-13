use super::*;

fn context(source: &str, tool: &str) -> ReceiptContext {
    ReceiptContext {
        schema: 1,
        source: serde_json::json!({"origin": source}),
        tool: serde_json::json!({"qualified": tool}),
    }
}

fn prediction(label: &[u8]) -> ActionPrediction {
    ActionPrediction {
        action: CacheDigest::blake3(label),
        invocation: CacheDigest::blake3(b"same invocation"),
        adapter: "rustc".into(),
        payload: "{}".into(),
    }
}

fn record(store: &Path, run: char, source: &str, tool: &str, predictions: Vec<ActionPrediction>) {
    let workspace = store.join("workspace");
    record_build_receipt(
        store,
        &run.to_string().repeat(64),
        &"a".repeat(64),
        &workspace,
        Some(&CargoBuildRoots {
            target_dir: workspace.join("target"),
            build_dir: workspace.join("build"),
        }),
        Some("group"),
        predictions,
        Some(&context(source, tool)),
        None,
    )
    .unwrap();
}

#[test]
fn empty_latest_retains_original_source_and_invocation_authority() {
    let store = tempfile::tempdir().unwrap();
    record(
        store.path(),
        'b',
        "old",
        "qualified",
        vec![prediction(b"old")],
    );
    record(store.path(), 'c', "new", "qualified", vec![]);
    let selected =
        checkout_receipt_evidence(store.path(), &store.path().join("workspace")).unwrap();
    assert_eq!(selected.len(), 2);
    assert!(
        selected
            .iter()
            .any(|value| value.context == Some(context("old", "qualified"))
                && !value.predictions.is_empty())
    );
    assert!(
        selected
            .iter()
            .any(|value| value.context == Some(context("new", "qualified"))
                && value.predictions.is_empty())
    );
    assert_eq!(
        group_receipt_evidence(store.path(), "group").unwrap(),
        selected
    );
}

#[test]
fn historical_tool_context_is_not_relabelled_as_current() {
    let store = tempfile::tempdir().unwrap();
    record(
        store.path(),
        'b',
        "old",
        "old tool",
        vec![prediction(b"old")],
    );
    record(store.path(), 'c', "new", "new tool", vec![]);
    let selected =
        checkout_receipt_evidence(store.path(), &store.path().join("workspace")).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(stored_receipt_evidence(store.path()).unwrap().len(), 2);
    assert_eq!(
        group_receipt_evidence(store.path(), "group").unwrap().len(),
        2
    );
}

#[test]
fn immutable_evidence_rejects_rewritten_or_noncanonical_records() {
    let store = tempfile::tempdir().unwrap();
    record(store.path(), 'b', "old", "tool", vec![prediction(b"old")]);
    let path = walk_files(&store.path().join(BUILD_RECEIPTS_DIR).join("evidence"))
        .unwrap()
        .pop()
        .unwrap()
        .path;
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(b'\n');
    std::fs::write(&path, bytes).unwrap();
    assert!(stored_receipt_evidence(store.path()).is_err());
}

#[test]
fn portable_projection_preserves_context_and_root_layout() {
    let store = tempfile::tempdir().unwrap();
    record(store.path(), 'b', "old", "tool", vec![prediction(b"old")]);
    let original = stored_receipt_evidence(store.path()).unwrap();
    let mut relocated = original.clone();
    let workspace = store.path().join("relocated/workspace");
    relocated[0].workspace = WorkspaceRoots {
        workspace_root: workspace.clone(),
        cargo: CargoBuildRoots {
            target_dir: workspace.join("target"),
            build_dir: workspace.join("build"),
        },
    };
    assert_eq!(
        semantic_receipt_evidence(&original).unwrap(),
        semantic_receipt_evidence(&relocated).unwrap()
    );
    relocated[0].context = Some(context("different source", "tool"));
    assert_ne!(
        semantic_receipt_evidence(&original).unwrap(),
        semantic_receipt_evidence(&relocated).unwrap()
    );
}

#[test]
fn original_prediction_must_match_native_action_owner() {
    let store = tempfile::tempdir().unwrap();
    record(store.path(), 'b', "old", "tool", vec![prediction(b"old")]);
    let records = stored_receipt_evidence(store.path()).unwrap();
    assert!(validate_evidence(&records, &BTreeMap::new()).is_err());
    let key = serde_json::to_string(&records[0].predictions[0].action).unwrap();
    assert!(
        validate_evidence(&records, &BTreeMap::from([(key.clone(), "rustdoc".into())])).is_err()
    );
    validate_evidence(&records, &BTreeMap::from([(key, "rustc".into())])).unwrap();
}

#[cfg(unix)]
#[test]
fn empty_run_export_and_import_preserve_original_evidence() {
    let source = tempfile::tempdir().unwrap();
    let action = CacheDigest::blake3(b"old");
    LocalCas::new(source.path())
        .store_bytes(&action, b"old")
        .unwrap();
    mbx_cache_core::LocalActionCache::new(source.path())
        .store(&RemoteActionResult {
            version: 1,
            action,
            metadata: None,
            output_root: None,
        })
        .unwrap();
    record(
        source.path(),
        'b',
        "old source",
        "tool",
        vec![prediction(b"old")],
    );
    record(source.path(), 'c', "new source", "tool", vec![]);
    let bundle = source.path().join("bundle");
    export_checkout_as(
        source.path(),
        &source.path().join("workspace"),
        &bundle,
        ExportAdditions::default(),
        ExportForm::Directory,
    )
    .unwrap();
    let verified = verify_directory_bundle(&bundle).unwrap();
    assert_eq!(verified.actions, 1);
    assert_eq!(verified.comparison.receipt_evidence.len(), 2);
    let destination = tempfile::tempdir().unwrap();
    import_archive(destination.path(), &bundle).unwrap();
    assert_eq!(
        stored_receipt_evidence(destination.path()).unwrap(),
        verified.comparison.receipt_evidence
    );
}

#[test]
fn malformed_roots_and_relocated_mutable_receipts_reject() {
    let store = tempfile::tempdir().unwrap();
    record(store.path(), 'b', "source", "tool", vec![]);
    let workspace = store.path().join("workspace");
    let mut value = stored_receipt_evidence(store.path()).unwrap().remove(0);
    for path in [
        "relative",
        "/workspace/../escape",
        "/workspace/./alias",
        "/workspace//alias",
    ] {
        value.workspace.workspace_root = PathBuf::from(path);
        assert!(value.validate().is_err(), "{path}");
    }
    let latest = latest_receipt_path(store.path(), &workspace);
    let mut bytes = std::fs::read(&latest).unwrap();
    bytes.push(b'\n');
    std::fs::write(&latest, bytes).unwrap();
    assert!(read_build_receipt(store.path(), &latest).is_none());
    let original = group_receipt_path(store.path(), "group", &"b".repeat(64));
    let foreign = group_receipt_path(store.path(), "group", &"c".repeat(64));
    std::fs::rename(original, &foreign).unwrap();
    assert!(read_build_receipt(store.path(), &foreign).is_none());
    assert!(
        group_receipt_evidence(store.path(), "group")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn mutable_receipt_rejects_old_versions_missing_run_context_and_foreign_extension() {
    let store = tempfile::tempdir().unwrap();
    record(store.path(), 'b', "source", "tool", vec![]);
    let latest = latest_receipt_path(store.path(), &store.path().join("workspace"));
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&latest).unwrap()).unwrap();
    for version in [1, 2, 3] {
        let mut value = original.clone();
        value["version"] = version.into();
        std::fs::write(&latest, mbx_cache_core::canonical_json(&value).unwrap()).unwrap();
        assert!(read_build_receipt(store.path(), &latest).is_none());
    }
    for field in ["run", "context", "lineage"] {
        let mut value = original.clone();
        value.as_object_mut().unwrap().remove(field);
        std::fs::write(&latest, mbx_cache_core::canonical_json(&value).unwrap()).unwrap();
        assert!(
            read_build_receipt(store.path(), &latest).is_none(),
            "{field}"
        );
    }
    let mut invalid_group = original;
    invalid_group["group"] = "invalid\ngroup".into();
    std::fs::write(
        &latest,
        mbx_cache_core::canonical_json(&invalid_group).unwrap(),
    )
    .unwrap();
    assert!(read_build_receipt(store.path(), &latest).is_none());
    let group = group_receipt_path(store.path(), "group", &"b".repeat(64));
    let foreign = group.with_extension("txt");
    std::fs::rename(group, &foreign).unwrap();
    assert!(read_build_receipt(store.path(), &foreign).is_none());
}

#[cfg(unix)]
#[test]
fn evidence_persistence_failure_preserves_original_bundle() {
    let source = tempfile::tempdir().unwrap();
    record(source.path(), 'b', "source", "tool", vec![]);
    let bundle = source.path().join("bundle");
    export_checkout_as(
        source.path(),
        &source.path().join("workspace"),
        &bundle,
        ExportAdditions::default(),
        ExportForm::Directory,
    )
    .unwrap();
    let before = verify_directory_bundle(&bundle).unwrap().physical_digest;
    let destination = tempfile::tempdir().unwrap();
    let ledger = destination.path().join(BUILD_RECEIPTS_DIR).join("evidence");
    std::fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    std::fs::write(ledger, b"blocking file").unwrap();
    assert!(import_archive(destination.path(), &bundle).is_err());
    assert_eq!(
        verify_directory_bundle(&bundle).unwrap().physical_digest,
        before
    );
    assert!(!destination.path().join(CAS_DIR).exists());
    assert!(!destination.path().join(ACTION_RESULTS_DIR).exists());
}
