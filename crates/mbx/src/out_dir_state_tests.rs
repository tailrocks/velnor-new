use super::*;
#[path = "out_dir_partition_tests.rs"]
mod partition_tests;

fn capture_owner(snapshot: &Snapshot, workspace: &WorkspaceRoots) -> Result<Vec<Snapshot>> {
    let store = tempfile::tempdir()?;
    let cas = mbx_cache_core::LocalCas::new(store.path());
    capture_owner_with_cas(snapshot, workspace, &cas)
}

fn capture_owner_with_cas(
    snapshot: &Snapshot,
    workspace: &WorkspaceRoots,
    cas: &mbx_cache_core::LocalCas,
) -> Result<Vec<Snapshot>> {
    let receipts = vec![owner_receipt(snapshot, workspace)];
    capture(
        &snapshot.root,
        workspace,
        &workspace.cargo.build_dir,
        cas,
        &receipts,
    )
}

fn owner_receipt(
    snapshot: &Snapshot,
    workspace: &WorkspaceRoots,
) -> mbx_cache_store::ReceiptEvidence {
    let predictions = snapshot
        .proofs
        .iter()
        .map(|proof| mbx_cache_core::ActionPrediction {
            invocation: proof.invocation.clone(),
            action: proof.action.clone(),
            adapter: "rustc".into(),
            payload: String::from_utf8(
                mbx_cache_core::canonical_json(&mbx_cache_rustc::RustcInputPrediction {
                    version: 4,
                    inputs: vec!["${out_dir}/generated-tool".into()],
                    environment: vec!["OUT_DIR".into()],
                    compiler_duration_ns: 0,
                    crate_name: "fixture".into(),
                })
                .expect("fixture serializes"),
            )
            .expect("canonical JSON is UTF-8"),
        })
        .collect();
    mbx_cache_store::ReceiptEvidence {
        lineage: None,
        workspace: workspace.clone(),
        identity: snapshot.proofs[0].identity.clone(),
        predictions,
        context: Some(snapshot.proofs[0].context.clone()),
    }
}

fn fixture() -> Result<(tempfile::TempDir, WorkspaceRoots, PathBuf, Snapshot)> {
    let directory = tempfile::tempdir()?;
    let build = directory.path().join("build");
    let source = build.join("unit/out");
    std::fs::create_dir_all(source.join("nested"))?;
    std::fs::write(
        source.join("nested/generated.rs"),
        b"pub const VALUE: u8 = 42;\n",
    )?;
    std::fs::write(source.join("generated-tool"), b"generated tool")?;
    let root = resolve_root(&directory.path().join("cache/out-dirs/v1"))?;
    let stable =
        stabilize(&source, &root)?.ok_or_else(|| eyre::eyre!("fixture cannot stabilize"))?;
    let snapshot = snapshot(
        PathBuf::from("unit/out"),
        &root,
        &stable,
        vec![Proof {
            identity: "a".repeat(64),
            invocation: CacheDigest::blake3(b"fixture-invocation"),
            action: CacheDigest::blake3(b"fixture-action"),
            context: mbx_cache_store::ReceiptContext {
                schema: 1,
                source: serde_json::json!({"repository":"fixture", "commit":"fixture"}),
                tool: serde_json::json!({"mbx":"fixture", "rust":"fixture"}),
            },
        }],
    )?;
    let workspace = WorkspaceRoots {
        workspace_root: directory.path().join("workspace"),
        cargo: CargoBuildRoots {
            target_dir: directory.path().join("target"),
            build_dir: build,
        },
    };
    Ok((directory, workspace, source, snapshot))
}

#[test]
fn hydration_preserves_stable_bytes_and_nanosecond_times() -> Result<()> {
    let (_directory, _workspace, source, snapshot) = fixture()?;
    let root = &snapshot.root;
    let stable = root.join(&snapshot.digest.hash);
    release_all_under(root);
    remove_tree(&stable)?;
    // A same-content build-script replay has newer source times than the immutable view.
    crate::materialize::set_modified(&source.join("generated-tool"), SystemTime::now())?;
    let restored = hydrate(&source, &snapshot, root)?;
    assert_eq!(restored, stable);
    assert_eq!(
        std::fs::read(restored.join("generated-tool"))?,
        b"generated tool"
    );
    for file in &snapshot.files {
        assert_eq!(
            std::fs::metadata(restored.join(&file.path))?.modified()?,
            modified(file)?
        );
    }
    release_all_under(root);
    Ok(())
}

#[test]
fn imported_owner_record_survives_fresh_capture() -> Result<()> {
    let (_directory, workspace, source, snapshot) = fixture()?;
    register(&snapshot.root, &workspace, &snapshot)?;
    let captured = capture_owner(&snapshot, &workspace)?;
    assert_eq!(
        serde_json::to_vec(&captured)?,
        serde_json::to_vec(&vec![snapshot.clone()])?
    );
    std::fs::write(source.join("generated-tool"), b"changed")?;
    assert!(!source_matches(&source, &snapshot, &snapshot.root)?);
    assert_eq!(capture_owner(&snapshot, &workspace)?.len(), 1);
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn changed_source_and_metadata_cannot_publish_views() -> Result<()> {
    let (_directory, _workspace, source, mut snapshot) = fixture()?;
    let root = snapshot.root.clone();
    let stable = root.join(&snapshot.digest.hash);
    release_all_under(&root);
    remove_tree(&stable)?;
    let original = std::fs::read(source.join("generated-tool"))?;
    std::fs::write(source.join("generated-tool"), b"changed")?;
    assert!(hydrate(&source, &snapshot, &root).is_err());
    assert!(!stable.exists());
    std::fs::write(source.join("generated-tool"), original)?;
    snapshot.files[0].modified_nanos = 1_000_000_000;
    assert!(hydrate(&source, &snapshot, &root).is_err());
    assert!(!stable.exists());
    Ok(())
}

#[test]
fn injection_and_owner_relocation_are_rejected() -> Result<()> {
    let (_directory, _workspace, source, snapshot) = fixture()?;
    for path in [
        "../escape",
        "/escape",
        "nested/../../escape",
        "nested\\escape",
        "nul\0path",
    ] {
        let mut injected = snapshot.clone();
        injected.source = path.into();
        assert!(validate_snapshot(&injected).is_err());
    }
    let mut injected = snapshot.clone();
    injected.files[0].path = "../escape".into();
    assert!(validate_snapshot(&injected).is_err());
    for root in [
        "/cache/./out-dirs/v1",
        "/cache//out-dirs/v1",
        "/cache/out-dirs/v1/",
    ] {
        let mut injected = snapshot.clone();
        injected.root = root.into();
        assert!(validate_snapshot(&injected).is_err());
    }
    assert!(hydrate(&source, &snapshot, &snapshot.root.join("foreign")).is_err());
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn missing_recorded_source_retains_proven_immutable_owner_closure() -> Result<()> {
    let (_directory, workspace, source, snapshot) = fixture()?;
    register(&snapshot.root, &workspace, &snapshot)?;
    std::fs::remove_dir_all(source)?;
    assert_eq!(capture_owner(&snapshot, &workspace)?.len(), 1);
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn cas_hydration_needs_complete_declared_objects_before_view_publication() -> Result<()> {
    let (directory, _workspace, source, snapshot) = fixture()?;
    let native = mbx_cache_core::LocalCas::new(directory.path().join("native-store"));
    cas::store_view(&native, &snapshot)?;
    release_all_under(&snapshot.root);
    remove_tree(&snapshot.root.join(&snapshot.digest.hash))?;
    std::fs::remove_dir_all(source)?;
    let missing = native.path_for(&snapshot.files[0].digest)?;
    let bytes = std::fs::read(&missing)?;
    std::fs::remove_file(&missing)?;
    assert!(hydrate_cas(&native, &snapshot, &snapshot.root).is_err());
    assert!(!snapshot.root.join(&snapshot.digest.hash).exists());
    native.store_bytes(&snapshot.files[0].digest, &bytes)?;
    let restored = hydrate_cas(&native, &snapshot, &snapshot.root)?;
    for file in &snapshot.files {
        assert!(file.digest.matches_file(&restored.join(&file.path))?);
        assert_eq!(
            std::fs::metadata(restored.join(&file.path))?.modified()?,
            modified(file)?
        );
    }
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn failed_finalization_cannot_certify_required_owned_output() -> Result<()> {
    let (_directory, workspace, _source, snapshot) = fixture()?;
    let error =
        capture_owner(&snapshot, &workspace).expect_err("missing journal must be unavailable");
    assert!(is_unavailable(&error));
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn missing_source_never_redirects_capture_through_foreign_journal_root() -> Result<()> {
    let (directory, workspace, source, snapshot) = fixture()?;
    register(&snapshot.root, &workspace, &snapshot)?;
    std::fs::remove_dir_all(source)?;
    let records =
        journal::journal_directory(&snapshot.root, &workspace, &snapshot.proofs[0].context)?;
    let record = std::fs::read_dir(records)?
        .next()
        .ok_or_else(|| eyre::eyre!("missing fixture journal"))??
        .path();
    let mut bytes: serde_json::Value = serde_json::from_slice(&std::fs::read(&record)?)?;
    let foreign = directory.path().join("foreign-owner");
    bytes["snapshot"]["root"] = serde_json::to_value(&foreign)?;
    std::fs::write(record, serde_json::to_vec(&bytes)?)?;
    assert!(snapshot.root.join(&snapshot.digest.hash).is_dir());
    let error = capture_owner(&snapshot, &workspace).expect_err("foreign root must be unavailable");
    assert!(is_unavailable(&error));
    assert!(!foreign.exists());
    let native = mbx_cache_core::LocalCas::new(directory.path().join("native-store"));
    cas::store_view(&native, &snapshot)?;
    release_all_under(&snapshot.root);
    remove_tree(&snapshot.root.join(&snapshot.digest.hash))?;
    let error = capture_owner_with_cas(&snapshot, &workspace, &native)
        .expect_err("complete CAS must not activate a foreign root");
    assert!(is_unavailable(&error));
    assert!(!foreign.exists());
    assert!(!snapshot.root.join(&snapshot.digest.hash).exists());
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn data_inventory_closes_owner_identity_without_materialization() -> Result<()> {
    let (_directory, _workspace, source, snapshot) = fixture()?;
    let directories = BTreeSet::from([PathBuf::from("nested")]);
    let mut files = BTreeMap::new();
    for file in &snapshot.files {
        files.insert(
            file.path.clone(),
            (CacheDigest::blake3_file(&source.join(&file.path))?, false),
        );
    }
    validate_inventory(&snapshot, &directories, &files)?;
    let file = files
        .get_mut(Path::new("generated-tool"))
        .ok_or_else(|| eyre::eyre!("missing fixture file"))?;
    file.0 = CacheDigest::blake3(b"changed");
    assert!(validate_inventory(&snapshot, &directories, &files).is_err());
    release_all_under(&snapshot.root);
    Ok(())
}

#[test]
fn leased_corrupt_view_is_never_replaced() -> Result<()> {
    let (_directory, _workspace, source, snapshot) = fixture()?;
    let stable = snapshot.root.join(&snapshot.digest.hash);
    let file = stable.join("generated-tool");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = std::fs::metadata(&file)?.permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&file, permissions)?;
    }
    std::fs::write(&file, b"corrupt")?;
    assert!(hydrate(&source, &snapshot, &snapshot.root).is_err());
    assert_eq!(std::fs::read(&file)?, b"corrupt");
    assert!(leased(&snapshot.root, &snapshot.digest.hash, false)?);
    release_all_under(&snapshot.root);
    Ok(())
}

#[cfg(unix)]
#[test]
fn changed_configured_owner_geometry_is_unavailable_before_journal_reads() -> Result<()> {
    let (directory, workspace, source, snapshot) = fixture()?;
    register(&snapshot.root, &workspace, &snapshot)?;
    std::fs::remove_dir_all(source)?;
    release_all_under(&snapshot.root);
    let displaced = snapshot.root.with_extension("displaced");
    std::fs::rename(&snapshot.root, &displaced)?;
    let foreign = directory.path().join("foreign-owner");
    std::fs::create_dir(&foreign)?;
    std::os::unix::fs::symlink(&foreign, &snapshot.root)?;
    let error = capture_owner(&snapshot, &workspace)
        .expect_err("changed owner geometry must be unavailable");
    assert!(is_unavailable(&error));
    assert_eq!(std::fs::read_dir(&foreign)?.count(), 0);
    std::fs::remove_file(&snapshot.root)?;
    std::fs::rename(displaced, &snapshot.root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_and_view_symlinks_are_rejected() -> Result<()> {
    let (directory, _workspace, source, snapshot) = fixture()?;
    let link = directory.path().join("linked");
    std::os::unix::fs::symlink(&source, &link)?;
    assert!(validate_source(&link, &snapshot, &snapshot.root).is_err());
    let root = &snapshot.root;
    let stable = root.join(&snapshot.digest.hash);
    release_all_under(root);
    remove_tree(&stable)?;
    std::os::unix::fs::symlink(&source, &stable)?;
    assert!(hydrate(&source, &snapshot, root).is_err());
    assert!(std::fs::symlink_metadata(&stable)?.file_type().is_symlink());
    release_all_under(root);
    Ok(())
}
