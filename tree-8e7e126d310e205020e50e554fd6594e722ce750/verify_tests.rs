use super::*;

fn bundle() -> (tempfile::TempDir, PathBuf, CacheDigest) {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    let cas = LocalCas::new(root);
    let action = CacheDigest::blake3(b"action");
    cas.store_bytes(&action, b"action").unwrap();
    mbx_cache_core::LocalActionCache::new(root)
        .store(&RemoteActionResult {
            version: 1,
            action: action.clone(),
            metadata: None,
            output_root: None,
        })
        .unwrap();
    let task = "a".repeat(64);
    record_build_receipt(
        root,
        &"b".repeat(64),
        &task,
        &root.join("workspace"),
        None,
        None,
        vec![ActionPrediction {
            action: action.clone(),
            invocation: CacheDigest::blake3(b"invocation"),
            adapter: "rustc".into(),
            payload: "{}".into(),
        }],
        None,
        None,
    )
    .unwrap();
    let bundle = root.join("bundle");
    export_checkout_as(
        root,
        &root.join("workspace"),
        &bundle,
        ExportAdditions::default(),
        ExportForm::Directory,
    )
    .unwrap();
    (source, bundle, action)
}

#[test]
fn verifies_without_consuming_or_changing_bundle_bytes_and_mtimes() {
    let (_source, root, _) = bundle();
    let before = physical_inventory(&root).unwrap();
    let mtimes = before
        .keys()
        .map(|name| {
            (
                name.clone(),
                std::fs::metadata(root.join(name))
                    .unwrap()
                    .modified()
                    .unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let verified = verify_directory_bundle(&root).unwrap();
    assert_eq!(verified.actions, 1);
    assert_eq!(verified.objects, 1);
    assert_eq!(verified.files, 3);
    assert_eq!(
        verified.physical_digest,
        verify_directory_bundle(&root).unwrap().physical_digest
    );
    for (name, modified) in mtimes {
        assert_eq!(
            std::fs::metadata(root.join(name))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
    }
}

#[test]
fn rejects_unused_cas_files_and_empty_foreign_directories() {
    let (_source, root, _) = bundle();
    let extra = CacheDigest::blake3(b"unreferenced");
    LocalCas::new(&root)
        .store_bytes(&extra, b"unreferenced")
        .unwrap();
    assert!(verify_directory_bundle(&root).is_err());
    std::fs::remove_file(LocalCas::new(&root).path_for(&extra).unwrap()).unwrap();
    let parent = LocalCas::new(&root).path_for(&extra).unwrap();
    if std::fs::read_dir(parent.parent().unwrap())
        .unwrap()
        .next()
        .is_none()
    {
        std::fs::remove_dir(parent.parent().unwrap()).unwrap();
    }
    std::fs::create_dir(root.join("foreign")).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
}

#[test]
fn rejects_corrupted_content_and_unknown_result_fields() {
    let (_source, root, action) = bundle();
    let object = LocalCas::new(&root).path_for(&action).unwrap();
    std::fs::write(&object, b"broken").unwrap();
    assert!(verify_directory_bundle(&root).is_err());
    std::fs::write(&object, b"action").unwrap();
    let result = mbx_cache_core::LocalActionCache::new(&root)
        .path_for(&action)
        .unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&result).unwrap()).unwrap();
    value["unknown"] = serde_json::json!(true);
    std::fs::write(result, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_hardlinks_and_symlink_bundle_roots() {
    let (_source, root, action) = bundle();
    let object = LocalCas::new(&root).path_for(&action).unwrap();
    let foreign = root.parent().unwrap().join("foreign");
    std::fs::write(&foreign, b"action").unwrap();
    std::fs::remove_file(&object).unwrap();
    std::os::unix::fs::symlink(&foreign, &object).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
    std::fs::remove_file(&object).unwrap();
    std::fs::hard_link(&foreign, &object).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
    std::fs::remove_file(&object).unwrap();
    std::fs::write(&object, b"action").unwrap();
    let link = root.parent().unwrap().join("bundle-link");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    assert!(verify_directory_bundle(&link).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_special_entries_without_reading_them() {
    let (_source, root, _) = bundle();
    let _socket = std::os::unix::net::UnixListener::bind(root.join("socket")).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
}

#[test]
fn rejects_unused_action_results_and_unknown_manifest_fields() {
    let (_source, root, action) = bundle();
    let result = mbx_cache_core::LocalActionCache::new(&root)
        .path_for(&action)
        .unwrap();
    let extra = result.with_file_name("foreign.json");
    std::fs::copy(&result, &extra).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
    std::fs::remove_file(extra).unwrap();
    let manifest = root.join(EXPORT_MANIFEST);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    value["foreign"] = serde_json::json!(true);
    std::fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(verify_directory_bundle(&root).is_err());
}

fn replace_result(
    root: &Path,
    action: &CacheDigest,
    metadata: Option<CacheDigest>,
    output_root: Option<CacheDigest>,
) {
    let cache = mbx_cache_core::LocalActionCache::new(root);
    let result = RemoteActionResult {
        version: 1,
        action: action.clone(),
        metadata,
        output_root,
    };
    std::fs::write(
        cache.path_for(action).unwrap(),
        mbx_cache_core::canonical_json(&result).unwrap(),
    )
    .unwrap();
}

#[test]
fn rejects_authenticated_directory_objects_with_unsupported_schema_or_paths() {
    for (version, name) in [(2, "artifact"), (1, "../escape"), (1, "/escape")] {
        let (_source, root, action) = bundle();
        let cas = LocalCas::new(&root);
        let contents = CacheDigest::blake3(b"artifact");
        cas.store_bytes(&contents, b"artifact").unwrap();
        let directory = CacheDirectory {
            version,
            files: vec![mbx_cache_core::CacheFileNode {
                digest: contents,
                executable: false,
                mode: 0o644,
                name: name.into(),
            }],
            directories: vec![],
            symlinks: vec![],
        };
        let bytes = mbx_cache_core::canonical_json(&directory).unwrap();
        let digest = CacheDigest::blake3(&bytes);
        cas.store_bytes(&digest, &bytes).unwrap();
        replace_result(&root, &action, None, Some(digest));
        let error = verify_directory_bundle(&root).unwrap_err();
        assert!(format!("{error:#}").contains("output directory is invalid"));
    }
}

#[test]
fn rejects_authenticated_metadata_with_foreign_kind_unknown_fields_or_version() {
    for (field, value) in [
        ("kind", serde_json::json!("foreign")),
        ("version", serde_json::json!(2)),
        ("unknown", serde_json::json!(true)),
    ] {
        let (_source, root, action) = bundle();
        let cas = LocalCas::new(&root);
        let empty = CacheDigest::blake3(b"");
        cas.store_bytes(&empty, b"").unwrap();
        let metadata = CapturedMetadata {
            version: 1,
            kind: mbx_cache_core::CapturedMetadataKind::Rustc,
            stdout: empty.clone(),
            stderr: empty,
        };
        let mut metadata = serde_json::to_value(metadata).unwrap();
        metadata[field] = value;
        let bytes = mbx_cache_core::canonical_json(&metadata).unwrap();
        let digest = CacheDigest::blake3(&bytes);
        cas.store_bytes(&digest, &bytes).unwrap();
        replace_result(&root, &action, Some(digest), None);
        let error = verify_directory_bundle(&root).unwrap_err();
        assert!(format!("{error:#}").contains("action metadata is invalid"));
    }
}

#[test]
fn rejects_valid_bytes_stored_under_a_foreign_cas_filename() {
    let (_source, root, action) = bundle();
    let object = LocalCas::new(&root).path_for(&action).unwrap();
    std::fs::rename(
        &object,
        object.with_file_name(format!("{}-{}", action.hash, action.size + 1)),
    )
    .unwrap();
    assert!(verify_directory_bundle(&root).is_err());
}

#[test]
fn binds_recognized_metadata_kind_to_the_prediction_owner() {
    for adapter in ["cc-path-binding-v1", "build-script", "foreign"] {
        let (_source, root, action) = bundle();
        let cas = LocalCas::new(&root);
        let empty = CacheDigest::blake3(b"");
        cas.store_bytes(&empty, b"").unwrap();
        let metadata = CapturedMetadata {
            version: 1,
            kind: CapturedMetadataKind::Rustc,
            stdout: empty.clone(),
            stderr: empty,
        };
        let bytes = mbx_cache_core::canonical_json(&metadata).unwrap();
        let digest = CacheDigest::blake3(&bytes);
        cas.store_bytes(&digest, &bytes).unwrap();
        replace_result(&root, &action, Some(digest), None);
        assert!(verify_directory_bundle(&root).is_ok());
        let manifest_path = root.join(EXPORT_MANIFEST);
        let mut manifest: ExportManifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest.tasks[0].predictions[0].adapter = adapter.into();
        manifest
            .action_owners
            .insert(serde_json::to_string(&action).unwrap(), adapter.into());
        std::fs::write(
            manifest_path,
            mbx_cache_core::canonical_json(&manifest).unwrap(),
        )
        .unwrap();
        let error = verify_directory_bundle(&root).unwrap_err();
        assert!(
            format!("{error:#}").contains("metadata"),
            "{adapter}: {error:#}"
        );
    }
}

#[cfg(unix)]
#[test]
fn physical_digest_commits_modes_but_ignores_mtimes() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_source, root, action) = bundle();
    let before = verify_directory_bundle(&root).unwrap().physical_digest;
    let object = LocalCas::new(&root).path_for(&action).unwrap();
    filetime::set_file_mtime(&object, filetime::FileTime::from_unix_time(1, 0)).unwrap();
    assert_eq!(
        before,
        verify_directory_bundle(&root).unwrap().physical_digest
    );
    let mode = std::fs::metadata(&object).unwrap().permissions().mode();
    std::fs::set_permissions(object, std::fs::Permissions::from_mode(mode ^ 0o100)).unwrap();
    assert_ne!(
        before,
        verify_directory_bundle(&root).unwrap().physical_digest
    );
}
