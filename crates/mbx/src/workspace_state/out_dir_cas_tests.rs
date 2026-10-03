use super::out_dir_tests::{capture, fixture, receipt_evidence, remove_readonly_tree};
use super::role_tests::{clear_roots, digest};
use super::*;

#[test]
fn absent_original_owned_output_roundtrips_cas_without_inventing_build_files() -> Result<()> {
    for shape in 0..4 {
        let store = tempfile::tempdir()?;
        let (roots, config, snapshot) = fixture(&store.path().join("source"), shape)?;
        std::fs::remove_dir_all(roots.cargo.build_dir.join(&snapshot.source))?;
        let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
        assert!(additions.objects.contains(&snapshot.files[0].digest));
        assert_eq!(
            referenced_objects(store.path(), digest(&additions)?)?,
            additions.objects
        );
        clear_roots(&roots.cargo)?;
        let stable = snapshot.root.join(&snapshot.digest.hash);
        remove_readonly_tree(&stable)?;
        assert!(matches!(
            restore(
                &config,
                store.path(),
                digest(&additions)?,
                &roots.workspace_root,
                &roots.cargo
            )?,
            RestoreOutcome::Restored { .. }
        ));
        assert!(!roots.cargo.build_dir.join(&snapshot.source).exists());
        assert_eq!(
            std::fs::read(stable.join("generated-tool"))?,
            b"#!/bin/sh\nexit 0\n"
        );
        assert_eq!(
            std::fs::metadata(stable.join("generated-tool"))?.modified()?,
            UNIX_EPOCH + std::time::Duration::new(123, 456)
        );
        let next = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
        assert_eq!(
            semantic_inventory(store.path(), Some(digest(&additions)?))?,
            semantic_inventory(store.path(), Some(digest(&next)?))?
        );
    }
    Ok(())
}

#[test]
fn corrupt_owner_cas_fails_pure_verify_without_publishing_cargo_roots() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, config, snapshot) = fixture(&store.path().join("source"), 1)?;
    std::fs::remove_dir_all(roots.cargo.build_dir.join(&snapshot.source))?;
    let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
    clear_roots(&roots.cargo)?;
    remove_readonly_tree(&snapshot.root.join(&snapshot.digest.hash))?;
    let cas = LocalCas::new(store.path());
    let path = cas
        .find(&snapshot.files[0].digest)?
        .ok_or_else(|| eyre::eyre!("missing owned file CAS"))?;
    std::fs::write(path, b"corrupt")?;
    assert!(referenced_objects(store.path(), digest(&additions)?).is_err());
    assert!(
        restore(
            &config,
            store.path(),
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )
        .is_err()
    );
    assert!(!roots.cargo.target_dir.exists());
    assert!(!roots.cargo.build_dir.exists());
    assert!(!snapshot.root.join(&snapshot.digest.hash).exists());
    Ok(())
}

#[test]
fn required_owner_proof_missing_from_selected_receipt_is_typed_unavailable() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, _, snapshot) = fixture(&store.path().join("source"), 1)?;
    let mut evidence = receipt_evidence(&roots)?;
    evidence.identity = "b".repeat(64);
    let mut prediction: mbx_cache_rustc::RustcInputPrediction =
        serde_json::from_str(&evidence.predictions[0].payload)?;
    prediction.environment.push("OUT_DIR".into());
    evidence.predictions[0].payload =
        String::from_utf8(mbx_cache_core::canonical_json(&prediction)?)?;
    assert!(matches!(
        super::capture(
            &Config::for_test(store.path()),
            store.path(),
            std::slice::from_ref(&roots),
            &snapshot.root,
            &[evidence]
        )?,
        CaptureOutcome::UnavailableOwnerProof { .. }
    ));
    assert!(roots.cargo.build_dir.join(&snapshot.source).exists());
    Ok(())
}

#[test]
fn historical_owned_views_share_source_without_overwriting_current_build_bytes() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, config, old) = fixture(&store.path().join("source"), 1)?;
    let source = roots.cargo.build_dir.join(&old.source);
    let bytes = b"#!/bin/sh\nexit 1\n";
    std::fs::write(source.join("generated-tool"), bytes)?;
    let stable = crate::out_dir::stabilize(&source, &old.root)?
        .ok_or_else(|| eyre::eyre!("cannot stabilize changed fixture"))?;
    let mut current = old.clone();
    current.files[0].digest = CacheDigest::blake3(bytes);
    let kind = if cfg!(unix) { "x" } else { "f" };
    current.digest = CacheDigest::blake3(
        format!(
            "{kind} {} 14:generated-tool\n",
            current.files[0].digest.hash
        )
        .as_bytes(),
    );
    let modified = std::fs::metadata(stable.join("generated-tool"))?
        .modified()?
        .duration_since(UNIX_EPOCH)?;
    current.files[0].modified_secs = modified.as_secs();
    current.files[0].modified_nanos = modified.subsec_nanos();
    current.proofs[0].action = CacheDigest::blake3(b"changed-action");
    crate::out_dir::register(&current.root, &roots, &current)?;
    let mut evidence = receipt_evidence(&roots)?;
    let mut prediction = evidence.predictions[0].clone();
    prediction.action = current.proofs[0].action.clone();
    evidence.predictions.push(prediction);
    let evidence = super::tests::persisted_evidence(store.path(), &[evidence])?;
    let CaptureOutcome::Captured(additions) = super::capture(
        &Config::for_test(store.path()),
        store.path(),
        std::slice::from_ref(&roots),
        &old.root,
        &evidence,
    )?
    else {
        bail!("historical view capture unavailable")
    };
    assert_eq!(
        semantic_inventory(store.path(), Some(digest(&additions)?))?
            .values()
            .filter(|value| value["type"] == "owned_out_dir")
            .count(),
        2
    );
    clear_roots(&roots.cargo)?;
    remove_readonly_tree(&old.root.join(&old.digest.hash))?;
    remove_readonly_tree(&current.root.join(&current.digest.hash))?;
    assert!(matches!(
        restore(
            &config,
            store.path(),
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    assert_eq!(
        std::fs::read(
            roots
                .cargo
                .build_dir
                .join(&old.source)
                .join("generated-tool")
        )?,
        bytes
    );
    assert_eq!(
        std::fs::read(old.root.join(&old.digest.hash).join("generated-tool"))?,
        b"#!/bin/sh\nexit 0\n"
    );
    assert_eq!(
        std::fs::read(
            current
                .root
                .join(&current.digest.hash)
                .join("generated-tool")
        )?,
        bytes
    );
    Ok(())
}

#[test]
fn data_only_receipt_linkage_rejects_foreign_proofs_and_uncovered_required_actions() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, _, snapshot) = fixture(&store.path().join("source"), 1)?;
    let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
    let mut evidence = receipt_evidence(&roots)?;
    validate_receipt_evidence(store.path(), digest(&additions)?, &[evidence.clone()])?;
    evidence.predictions[0].action = CacheDigest::blake3(b"foreign action");
    assert!(validate_receipt_evidence(store.path(), digest(&additions)?, &[evidence]).is_err());
    let missing = super::out_dir_tests::replace_bundle(store.path(), &additions, |bundle| {
        bundle.workspaces[0].owned_out_dirs.clear();
    })?;
    let mut evidence = receipt_evidence(&roots)?;
    let mut prediction: mbx_cache_rustc::RustcInputPrediction =
        serde_json::from_str(&evidence.predictions[0].payload)?;
    prediction.environment.push("OUT_DIR".into());
    evidence.predictions[0].payload =
        String::from_utf8(mbx_cache_core::canonical_json(&prediction)?)?;
    assert!(validate_receipt_evidence(store.path(), &missing, &[evidence]).is_err());
    Ok(())
}

#[test]
fn absent_original_and_owner_bytes_are_typed_unavailable() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, _, snapshot) = fixture(&store.path().join("source"), 1)?;
    std::fs::remove_dir_all(roots.cargo.build_dir.join(&snapshot.source))?;
    remove_readonly_tree(&snapshot.root.join(&snapshot.digest.hash))?;
    let evidence = receipt_evidence(&roots)?;
    assert!(matches!(
        super::capture(
            &Config::for_test(store.path()),
            store.path(),
            std::slice::from_ref(&roots),
            &snapshot.root,
            &[evidence]
        )?,
        CaptureOutcome::UnavailableOwnerProof { .. }
    ));
    assert!(!roots.cargo.build_dir.join(&snapshot.source).exists());
    assert!(!snapshot.root.join(&snapshot.digest.hash).exists());
    Ok(())
}

#[test]
fn foreign_owner_context_proof_cannot_publish_cargo_roots() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, config, snapshot) = fixture(&store.path().join("source"), 1)?;
    let additions = capture(store.path(), std::slice::from_ref(&roots), &snapshot.root)?;
    let foreign = super::out_dir_tests::replace_bundle(store.path(), &additions, |bundle| {
        bundle.workspaces[0].owned_out_dirs[0].proofs[0]
            .context
            .source = serde_json::json!({"fixture": "foreign-source"});
    })?;
    clear_roots(&roots.cargo)?;
    assert!(
        restore(
            &config,
            store.path(),
            &foreign,
            &roots.workspace_root,
            &roots.cargo
        )
        .is_err()
    );
    assert!(!roots.cargo.target_dir.exists());
    assert!(!roots.cargo.build_dir.exists());
    Ok(())
}
