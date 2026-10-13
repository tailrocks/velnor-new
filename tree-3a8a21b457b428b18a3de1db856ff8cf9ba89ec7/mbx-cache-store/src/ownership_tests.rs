use super::*;

fn native_action(root: &Path, label: &[u8], kind: CapturedMetadataKind) -> CacheDigest {
    let cas = LocalCas::new(root);
    let action = CacheDigest::blake3(label);
    cas.store_bytes(&action, label).unwrap();
    let empty = CacheDigest::blake3(b"");
    cas.store_bytes(&empty, b"").unwrap();
    let metadata = CapturedMetadata {
        version: 1,
        kind,
        stdout: empty.clone(),
        stderr: empty,
    };
    let bytes = mbx_cache_core::canonical_json(&metadata).unwrap();
    let metadata = CacheDigest::blake3(&bytes);
    cas.store_bytes(&metadata, &bytes).unwrap();
    mbx_cache_core::LocalActionCache::new(root)
        .store(&RemoteActionResult {
            version: 1,
            action: action.clone(),
            metadata: Some(metadata),
            output_root: None,
        })
        .unwrap();
    action
}

fn receipt(root: &Path, action: CacheDigest, completed_nanos: u64, adapter: &str) -> BuildReceipt {
    BuildReceipt {
        version: BUILD_RECEIPT_VERSION,
        lineage: None,
        run: "b".repeat(64),
        workspace_root: root.join("workspace"),
        cargo: None,
        identity: "a".repeat(64),
        completed_nanos,
        context: None,
        group: None,
        predictions: vec![ActionPrediction {
            action,
            invocation: CacheDigest::blake3(b"same invocation"),
            adapter: adapter.into(),
            payload: "{}".into(),
        }],
    }
}

fn export(
    root: &Path,
    receipts: Vec<BuildReceipt>,
    name: &str,
    retained: Option<&ComparisonState>,
) -> PathBuf {
    let bundle = root.join(name);
    export_receipts(
        root,
        receipts,
        &bundle,
        ExportAdditions::default(),
        ExportForm::Directory,
        ExportPolicy {
            retained,
            ..ExportPolicy::default()
        },
        None,
    )
    .unwrap();
    bundle
}

#[test]
fn ownership_survives_group_prediction_deduplication() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let old = native_action(root, b"old action", CapturedMetadataKind::Rustc);
    let new = native_action(root, b"new action", CapturedMetadataKind::Rustc);
    let bundle = export(
        root,
        vec![
            receipt(root, old.clone(), 1, "rustc"),
            receipt(root, new.clone(), 2, "rustc"),
        ],
        "group",
        None,
    );
    let verified = verify_directory_bundle(&bundle).unwrap();
    assert_eq!(verified.actions, 2);
    assert_eq!(verified.comparison.predictions.len(), 1);
    for action in [&old, &new] {
        assert_eq!(
            verified
                .comparison
                .action_owners
                .get(&serde_json::to_string(action).unwrap())
                .map(String::as_str),
            Some("rustc")
        );
    }
}

#[test]
fn comparison_rejects_equivalent_noncanonical_prediction_strings() {
    let source = tempfile::tempdir().unwrap();
    let action = native_action(source.path(), b"action", CapturedMetadataKind::Rustc);
    let bundle = export(
        source.path(),
        vec![receipt(source.path(), action, 1, "rustc")],
        "bundle",
        None,
    );
    let mut comparison = ComparisonState::from_directory(&bundle).unwrap();
    let canonical = comparison.predictions.pop_first().unwrap();
    let value: serde_json::Value = serde_json::from_str(&canonical).unwrap();
    comparison
        .predictions
        .insert(serde_json::to_string_pretty(&value).unwrap());
    assert!(comparison.validate().is_err());
}

#[test]
fn ownership_survives_retained_result_without_a_live_prediction() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let old = native_action(root, b"old action", CapturedMetadataKind::Rustc);
    let first = export(
        root,
        vec![receipt(root, old.clone(), 1, "rustc")],
        "first",
        None,
    );
    let baseline = verify_directory_bundle(&first).unwrap().comparison;
    let new = native_action(root, b"new action", CapturedMetadataKind::Rustc);
    let second = export(
        root,
        vec![receipt(root, new, 2, "rustc")],
        "second",
        Some(&baseline),
    );
    let verified = verify_directory_bundle(&second).unwrap();
    assert_eq!(verified.comparison.action_results.len(), 2);
    assert_eq!(verified.comparison.action_owners.len(), 2);
    assert_eq!(verified.comparison.predictions.len(), 1);
    assert!(
        verified
            .comparison
            .action_owners
            .contains_key(&serde_json::to_string(&old).unwrap())
    );
}

#[test]
fn source_receipts_cannot_assign_conflicting_owners_to_an_action() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let action = native_action(root, b"same action", CapturedMetadataKind::Rustc);
    let error = export_receipts(
        root,
        vec![
            receipt(root, action.clone(), 1, "rustc"),
            receipt(root, action, 2, "cc-path-binding-v1"),
        ],
        &root.join("conflict"),
        ExportAdditions::default(),
        ExportForm::Directory,
        ExportPolicy::default(),
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("conflicting native owners"));
    assert!(!root.join("conflict").exists());
}

#[test]
fn comparison_rejects_missing_extra_foreign_and_mismatched_owners() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let action = native_action(root, b"action", CapturedMetadataKind::Rustc);
    let bundle = export(
        root,
        vec![receipt(root, action.clone(), 1, "rustc")],
        "bundle",
        None,
    );
    let baseline = verify_directory_bundle(&bundle).unwrap().comparison;
    let serialized = serde_json::to_value(&baseline).unwrap();
    for mutation in ["missing", "extra", "foreign", "mismatched", "old-version"] {
        let mut value = serialized.clone();
        let key = serde_json::to_string(&action).unwrap();
        match mutation {
            "missing" => value["action_owners"] = serde_json::json!({}),
            "extra" => value["action_owners"]["foreign"] = serde_json::json!("rustc"),
            "foreign" => value["action_owners"][&key] = serde_json::json!("foreign"),
            "mismatched" => value["action_owners"][&key] = serde_json::json!("cc-path-binding-v1"),
            "old-version" => value["version"] = serde_json::json!(1),
            _ => unreachable!(),
        }
        let state: ComparisonState = serde_json::from_value(value).unwrap();
        assert!(state.validate().is_err(), "{mutation}");
    }
}

#[test]
fn export_manifest_rejects_legacy_versions_and_missing_registry() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let action = native_action(root, b"action", CapturedMetadataKind::Rustc);
    let bundle = export(
        root,
        vec![receipt(root, action, 1, "rustc")],
        "bundle",
        None,
    );
    let path = bundle.join(EXPORT_MANIFEST);
    let original = std::fs::read(&path).unwrap();
    for version in [1, 2, 3, 4] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value["version"] = serde_json::json!(version);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(verify_directory_bundle(&bundle).is_err());
    }
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value.as_object_mut().unwrap().remove("action_owners");
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(verify_directory_bundle(&bundle).is_err());
}

#[test]
fn export_manifest_rejects_duplicate_keys_and_noncanonical_bytes() {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let action = native_action(root, b"action", CapturedMetadataKind::Rustc);
    let bundle = export(
        root,
        vec![receipt(root, action, 1, "rustc")],
        "bundle",
        None,
    );
    let path = bundle.join(EXPORT_MANIFEST);
    let original = std::fs::read(&path).unwrap();
    let mut noncanonical = original.clone();
    noncanonical.push(b'\n');
    std::fs::write(&path, noncanonical).unwrap();
    assert!(verify_directory_bundle(&bundle).is_err());
    let mut duplicated = original;
    duplicated.pop();
    duplicated.extend_from_slice(b",\"version\":5}");
    std::fs::write(path, duplicated).unwrap();
    assert!(verify_directory_bundle(&bundle).is_err());
}

#[test]
fn owned_paths_cover_every_native_namespace_without_claiming_store_siblings() {
    let root = Path::new("store");
    let owned = owned_paths(root);
    assert_eq!(owned.len(), 8);
    for path in [
        CAS_DIR,
        ACTION_RESULTS_DIR,
        CHECKOUTS_DIR,
        BUILD_RECEIPTS_DIR,
        IMPORT_STAGING_DIR,
        "task-manifests/v1",
        "sessions/v1",
        "gc/v1",
    ] {
        assert!(owned.contains(&root.join(path)));
    }
    assert!(
        !owned
            .iter()
            .any(|path| root.join("build").starts_with(path))
    );
}

#[test]
fn exports_and_verifies_every_actual_native_prediction_owner_token() {
    for (adapter, kind) in [
        ("rustc", CapturedMetadataKind::Rustc),
        ("cc-path-binding-v1", CapturedMetadataKind::Cc),
        ("build-script", CapturedMetadataKind::BuildScript),
        ("rustdoc", CapturedMetadataKind::Rustc),
    ] {
        let source = tempfile::tempdir().unwrap();
        let root = source.path();
        let action = native_action(root, adapter.as_bytes(), kind);
        let bundle = export(
            root,
            vec![receipt(root, action.clone(), 1, adapter)],
            "bundle",
            None,
        );
        let verified = verify_directory_bundle(&bundle).unwrap();
        assert_eq!(
            verified
                .comparison
                .action_owners
                .get(&serde_json::to_string(&action).unwrap())
                .map(String::as_str),
            Some(adapter)
        );
    }
}
