use super::role_tests::{clear_roots, digest, roots_fixture};
use super::*;

pub(super) fn capture(
    store: &Path,
    targets: &[WorkspaceRoots],
    owner: &Path,
) -> Result<ExportAdditions> {
    let evidence = targets
        .iter()
        .map(receipt_evidence)
        .collect::<Result<Vec<_>>>()?;
    let evidence = super::tests::persisted_evidence(store, &evidence)?;
    match super::capture(&Config::for_test(store), store, targets, owner, &evidence)? {
        CaptureOutcome::Captured(additions) | CaptureOutcome::RetainedOwner { additions, .. } => {
            Ok(additions)
        }
        CaptureOutcome::UnavailableManagedOverlap
        | CaptureOutcome::UnavailableOwnerProof { .. } => bail!("unsupported fixture capture"),
    }
}

pub(super) fn receipt_evidence(roots: &WorkspaceRoots) -> Result<mbx_cache_store::ReceiptEvidence> {
    let payload = String::from_utf8(mbx_cache_core::canonical_json(
        &mbx_cache_rustc::RustcInputPrediction {
            version: 4,
            inputs: vec![],
            environment: vec![],
            compiler_duration_ns: 0,
            crate_name: String::new(),
        },
    )?)?;
    Ok(mbx_cache_store::ReceiptEvidence {
        lineage: None,
        workspace: roots.clone(),
        identity: "a".repeat(64),
        context: Some(fixture_context()),
        predictions: vec![mbx_cache_core::ActionPrediction {
            invocation: CacheDigest::blake3(b"fixture-invocation"),
            action: CacheDigest::blake3(b"fixture-action"),
            adapter: "rustc".into(),
            payload,
        }],
    })
}

pub(super) fn fixture_context() -> mbx_cache_store::ReceiptContext {
    mbx_cache_store::ReceiptContext {
        schema: 1,
        source: serde_json::json!({"fixture": "source"}),
        tool: serde_json::json!({"fixture": "tool"}),
    }
}

pub(super) fn fixture(
    base: &Path,
    shape: u8,
) -> Result<(WorkspaceRoots, Config, crate::out_dir::Snapshot)> {
    let roots = roots_fixture(base, shape)?;
    let source = roots.cargo.build_dir.join("unit/out");
    std::fs::create_dir_all(&source)?;
    let bytes = b"#!/bin/sh\nexit 0\n";
    let helper = source.join("generated-tool");
    std::fs::write(&helper, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))?;
    }
    filetime::set_file_mtime(&helper, filetime::FileTime::from_unix_time(123, 456))?;
    let config = Config::for_test(&base.join("cache"));
    let root = crate::out_dir::resolve_root(&config.cache_dir.join(crate::out_dir::ROOT))?;
    let stable = crate::out_dir::stabilize(&source, &root)?
        .ok_or_else(|| eyre::eyre!("fixture cannot stabilize"))?;
    let kind = if cfg!(unix) { "x" } else { "f" };
    let manifest = format!(
        "{kind} {} 14:generated-tool\n",
        CacheDigest::blake3(bytes).hash
    );
    let snapshot: crate::out_dir::Snapshot = serde_json::from_value(serde_json::json!({
        "source": "unit/out", "root": root, "digest": CacheDigest::blake3(manifest.as_bytes()),
        "directories": [],
        "files": [{"path": "generated-tool", "digest": CacheDigest::blake3(bytes),
            "executable": cfg!(unix), "modified_secs": 123, "modified_nanos": 456}],
        "proofs": [{"identity": "a".repeat(64), "context": fixture_context(),
            "invocation": CacheDigest::blake3(b"fixture-invocation"),
            "action": CacheDigest::blake3(b"fixture-action")}],
    }))?;
    assert_eq!(stable, snapshot.root.join(&snapshot.digest.hash));
    crate::out_dir::register(&snapshot.root, &roots, &snapshot)?;
    Ok((roots, config, snapshot))
}

pub(super) fn remove_readonly_tree(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for entry in std::fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                remove_readonly_tree(&path)?;
            }
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    {
        for entry in std::fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                remove_readonly_tree(&path)?;
            } else {
                let mut permissions = std::fs::metadata(&path)?.permissions();
                permissions.set_readonly(false);
                std::fs::set_permissions(path, permissions)?;
            }
        }
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions)?;
    }
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[test]
fn native_owned_views_roundtrip_all_role_layouts_and_reexport() -> Result<()> {
    for shape in 0..4 {
        let store = tempfile::tempdir()?;
        let (roots, config, snapshot) = fixture(&store.path().join("source"), shape)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
        assert_eq!(
            referenced_objects(store.path(), digest(&additions)?)?,
            additions.objects
        );
        // Stable view times must stay independent of a replayed source timestamp.
        filetime::set_file_mtime(
            roots.cargo.build_dir.join("unit/out/generated-tool"),
            filetime::FileTime::from_unix_time(999, 999),
        )?;
        let fresh = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
        assert_eq!(
            semantic_inventory(store.path(), Some(digest(&additions)?))?,
            semantic_inventory(store.path(), Some(digest(&fresh)?))?
        );
        clear_roots(&roots.cargo)?;
        let stable = snapshot.root.join(&snapshot.digest.hash);
        remove_readonly_tree(&stable)?;
        // Pure verification needs neither live Cargo roots nor activated owner views.
        assert_eq!(
            referenced_objects(store.path(), digest(&fresh)?)?,
            fresh.objects
        );
        assert!(!stable.exists());
        assert!(!roots.cargo.target_dir.exists());
        assert!(!roots.cargo.build_dir.exists());
        assert!(matches!(
            restore(
                &config,
                store.path(),
                digest(&fresh)?,
                &roots.workspace_root,
                &roots.cargo
            )?,
            RestoreOutcome::Restored { .. }
        ));
        assert_eq!(
            std::fs::read(stable.join("generated-tool"))?,
            b"#!/bin/sh\nexit 0\n"
        );
        #[cfg(unix)]
        assert!(
            std::process::Command::new(stable.join("generated-tool"))
                .status()?
                .success()
        );
        assert_eq!(
            std::fs::metadata(stable.join("generated-tool"))?.modified()?,
            UNIX_EPOCH + std::time::Duration::new(123, 456)
        );
        let after = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
        assert_eq!(
            semantic_inventory(store.path(), Some(digest(&fresh)?))?,
            semantic_inventory(store.path(), Some(digest(&after)?))?
        );
    }
    Ok(())
}

pub(super) fn replace_bundle(
    store: &Path,
    additions: &ExportAdditions,
    change: impl FnOnce(&mut Bundle),
) -> Result<CacheDigest> {
    let cas = LocalCas::new(store);
    let path = cas
        .find(digest(additions)?)?
        .ok_or_else(|| eyre::eyre!("missing bundle"))?;
    let mut bundle: Bundle = serde_json::from_slice(&std::fs::read(path)?)?;
    change(&mut bundle);
    let bytes = mbx_cache_core::canonical_json(&bundle)?;
    let digest = CacheDigest::blake3(&bytes);
    cas.store_bytes(&digest, &bytes)?;
    Ok(digest)
}

#[test]
fn invalid_owned_descriptors_fail_pure_verification_before_role_publication() -> Result<()> {
    for failure in 0..4 {
        let store = tempfile::tempdir()?;
        let (roots, config, snapshot) = fixture(&store.path().join("source"), 1)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
        let corrupt = replace_bundle(store.path(), &additions, |bundle| {
            let descriptor = &mut bundle.workspaces[0].owned_out_dirs[0];
            match failure {
                0 => descriptor.digest = CacheDigest::blake3(b"wrong owner manifest"),
                1 => descriptor.source = PathBuf::from("../escaped"),
                2 => descriptor.files[0].modified_nanos = 1_000_000_000,
                _ => descriptor.files.clear(),
            }
        })?;
        clear_roots(&roots.cargo)?;
        assert!(referenced_objects(store.path(), &corrupt).is_err());
        assert!(
            restore(
                &config,
                store.path(),
                &corrupt,
                &roots.workspace_root,
                &roots.cargo
            )
            .is_err()
        );
        assert!(!roots.cargo.target_dir.exists());
        assert!(!roots.cargo.build_dir.exists());
    }
    Ok(())
}

#[test]
fn omitted_owner_descriptors_remain_in_retained_closed_attachment() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (first, _, snapshot) = fixture(&store.path().join("first"), 1)?;
    let second = roots_fixture(&store.path().join("second"), 0)?;
    let baseline = capture(store.path(), std::slice::from_ref(&first), &snapshot.root)?;
    let current = capture(store.path(), std::slice::from_ref(&second), &snapshot.root)?;
    let retained = retain(store.path(), current, Some(digest(&baseline)?))?.additions;
    assert_eq!(
        referenced_objects(store.path(), digest(&retained)?)?,
        retained.objects
    );
    let inventory = semantic_inventory(store.path(), Some(digest(&retained)?))?;
    assert!(
        inventory
            .values()
            .any(|value| value["type"] == "owned_out_dir")
    );
    Ok(())
}

#[test]
fn streaming_attachment_hash_matches_native_digest_and_returns_reader_errors() -> Result<()> {
    let bytes = vec![42; 200_000];
    assert_eq!(
        CacheDigest::blake3_reader(std::io::Cursor::new(&bytes))?,
        CacheDigest::blake3(&bytes)
    );
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fixture reader failure"))
        }
    }
    assert!(CacheDigest::blake3_reader(Broken).is_err());
    Ok(())
}

#[test]
fn foreign_configured_owner_root_refuses_before_any_cargo_publication() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, mut config, snapshot) = fixture(&store.path().join("source"), 1)?;
    let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
    clear_roots(&roots.cargo)?;
    config.cache_dir = store.path().join("foreign-cache");
    assert_eq!(
        restore(
            &config,
            store.path(),
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::SkippedUnavailable
    );
    assert!(!roots.cargo.target_dir.exists());
    assert!(!roots.cargo.build_dir.exists());
    assert!(!config.cache_dir.exists());
    assert_eq!(
        std::fs::read(
            snapshot
                .root
                .join(&snapshot.digest.hash)
                .join("generated-tool")
        )?,
        b"#!/bin/sh\nexit 0\n"
    );
    Ok(())
}

#[test]
fn owned_generated_bytes_never_use_abstract_mbx_replacement() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, _config, snapshot) = fixture(&store.path().join("source"), 1)?;
    let cas = LocalCas::new(store.path());
    let executable = CacheDigest::blake3(b"#!/bin/sh\nexit 0\n");
    let (physical, link) = resolve_roots(&roots)?;
    let state = capture::capture_workspace(
        &cas,
        &roots,
        &executable,
        &mut BTreeSet::new(),
        &snapshot.root,
        (&physical, link),
        &[receipt_evidence(&roots)?],
    )?;
    assert!(
        !state
            .trees
            .iter()
            .flat_map(|tree| &tree.references)
            .any(
                |reference| reference.path == Path::new("unit/out/generated-tool")
                    && matches!(reference.source, FileSource::Mbx)
            )
    );
    semantic_workspace(&cas, &state)?;
    Ok(())
}

#[test]
fn relative_native_cache_paths_restore_owned_views_from_invocation_cwd() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let base = tempfile::tempdir_in(&cwd)?;
    let store = base.path().join("actions");
    std::fs::create_dir_all(&store)?;
    let (roots, mut config, snapshot) = fixture(&base.path().join("source"), 1)?;
    let store = store.strip_prefix(&cwd)?.to_path_buf();
    config.cache_dir = config.cache_dir.strip_prefix(&cwd)?.to_path_buf();
    let owner = config.cache_dir.join(crate::out_dir::ROOT);
    let additions = capture(&store, std::slice::from_ref(&roots), &owner)?;
    clear_roots(&roots.cargo)?;
    let stable = snapshot.root.join(&snapshot.digest.hash);
    remove_readonly_tree(&stable)?;
    assert!(matches!(
        restore(
            &config,
            &store,
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    assert_eq!(
        std::fs::read(stable.join("generated-tool"))?,
        b"#!/bin/sh\nexit 0\n"
    );
    remove_readonly_tree(&stable)?;
    Ok(())
}
