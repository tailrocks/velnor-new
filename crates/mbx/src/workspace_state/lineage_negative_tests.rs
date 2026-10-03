use super::*;

#[test]
fn same_weak_signature_does_not_select_between_distinct_native_owners() -> Result<()> {
    let fixture = fixture()?;
    let separate = checkout(fixture._directory.path(), "separate")?;
    std::fs::create_dir_all(&separate.cargo.target_dir)?;
    std::fs::create_dir_all(&separate.cargo.build_dir)?;
    std::fs::write(
        separate.cargo.target_dir.join("artifact"),
        b"different owner",
    )?;
    let targets = [fixture.original.clone(), separate];
    let records = targets
        .iter()
        .flat_map(|roots| evidence(roots, None))
        .collect::<Vec<_>>();
    let records = publish_receipts(&fixture, &records)?;
    let CaptureOutcome::Captured(additions) = super::super::super::capture(
        &fixture.config,
        &fixture.store,
        &targets,
        &fixture.config.cache_dir.join(crate::out_dir::ROOT),
        &records,
    )?
    else {
        bail!("independent native captures must remain independent");
    };
    let bundle = state(&fixture, &additions)?;
    assert_eq!(bundle.workspaces.len(), 2);
    assert_ne!(bundle.workspaces[0].owner, bundle.workspaces[1].owner);
    assert_eq!(
        bundle.workspaces[0].signature,
        bundle.workspaces[1].signature
    );
    let clone = checkout(fixture._directory.path(), "clone")?;
    assert_eq!(
        super::super::super::restore(
            &fixture.config,
            &fixture.store,
            &additions.attachments[ATTACHMENT],
            &clone.workspace_root,
            &clone.cargo
        )?,
        RestoreOutcome::SkippedAmbiguous
    );
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
    assert!(
        !fixture
            .store
            .join("build-receipts/v4/local-grants")
            .exists()
    );
    Ok(())
}

#[test]
fn legacy_attachment_is_rejected_without_creating_a_local_capability() -> Result<()> {
    let fixture = fixture()?;
    let additions = capture(&fixture, &fixture.original, None)?;
    let mut bundle = state(&fixture, &additions)?;
    bundle.version = 3;
    let cas = LocalCas::new(&fixture.store);
    let digest = store_canonical(&cas, &bundle)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    assert!(
        super::super::super::restore(
            &fixture.config,
            &fixture.store,
            &digest,
            &clone.workspace_root,
            &clone.cargo
        )
        .is_err()
    );
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
    Ok(())
}

#[test]
fn original_none_context_and_nonrecursive_native_owner_closure_are_preserved() -> Result<()> {
    let fixture = fixture()?;
    let additions = capture(&fixture, &fixture.original, None)?;
    let bundle = state(&fixture, &additions)?;
    let cas = LocalCas::new(&fixture.store);
    let owner = &bundle.workspaces[0].owner;
    let anchor: OwnerAnchor = read_canonical(&cas, owner)?;
    assert!(
        anchor
            .original_receipts
            .iter()
            .all(|record| record.context.is_none() && record.lineage.is_none())
    );
    assert_eq!(anchor.origin, fixture.original);
    let (_, closure) = owner_objects(&cas, owner)?;
    let mut exact = snapshot_objects(&bundle.workspaces[0]);
    exact.insert(owner.clone());
    exact.insert(anchor.initial_snapshot);
    assert_eq!(closure, exact);
    assert_eq!(
        referenced_objects(&fixture.store, &additions.attachments[ATTACHMENT])?,
        additions.objects
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn owner_proof_budget_refusal_preserves_successful_materialization() -> Result<()> {
    for budget in [1, 31, 32] {
        let mut fixture = fixture()?;
        let additions = capture(&fixture, &fixture.original, None)?;
        fixture.config.gc.max_bytes = budget;
        let clone = checkout(fixture._directory.path(), "clone")?;
        assert!(matches!(
            super::super::super::restore(
                &fixture.config,
                &fixture.store,
                &additions.attachments[ATTACHMENT],
                &clone.workspace_root,
                &clone.cargo
            )?,
            RestoreOutcome::Restored { .. }
        ));
        assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
        assert!(
            !fixture
                .store
                .join("build-receipts/v4/local-grants")
                .exists()
        );
        assert_eq!(
            std::fs::read(clone.cargo.target_dir.join("artifact"))?,
            b"complete native artifact"
        );
        assert_eq!(
            std::fs::read(clone.cargo.build_dir.join("fingerprint"))?,
            b"native scheduler bytes"
        );
    }
    Ok(())
}

#[test]
fn canonical_changed_payload_cannot_reassign_an_immutable_owner() -> Result<()> {
    let fixture = fixture()?;
    let additions = capture(&fixture, &fixture.original, None)?;
    let mut bundle = state(&fixture, &additions)?;
    bundle.workspaces[0].trees[0].inline_files[0].mode ^= 0o111;
    let digest = store_canonical(&LocalCas::new(&fixture.store), &bundle)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    assert!(
        super::super::super::restore(
            &fixture.config,
            &fixture.store,
            &digest,
            &clone.workspace_root,
            &clone.cargo
        )
        .is_err()
    );
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
    Ok(())
}

#[test]
fn failed_destination_resolution_never_grants_a_local_owner() -> Result<()> {
    let fixture = fixture()?;
    let additions = capture(&fixture, &fixture.original, None)?;
    let mut clone = checkout(fixture._directory.path(), "clone")?;
    let blocked = fixture._directory.path().join("blocked-parent");
    std::fs::write(&blocked, b"cannot stage a directory here")?;
    clone.cargo.build_dir = blocked.join("build");
    assert!(
        super::super::super::restore(
            &fixture.config,
            &fixture.store,
            &additions.attachments[ATTACHMENT],
            &clone.workspace_root,
            &clone.cargo
        )
        .is_err()
    );
    assert!(!clone.cargo.target_dir.exists());
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn live_local_selector_chooses_only_its_owner_and_never_a_weak_fallback() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    super::super::super::restore(
        &fixture.config,
        &fixture.store,
        &initial.attachments[ATTACHMENT],
        &clone.workspace_root,
        &clone.cargo,
    )?;
    let owner = freeze_lineage(&fixture.config, &fixture.store, &clone)?
        .ok_or_else(|| eyre::eyre!("missing native selector"))?
        .owner;
    let separate = checkout(fixture._directory.path(), "separate")?;
    std::fs::create_dir_all(&separate.cargo.target_dir)?;
    std::fs::create_dir_all(&separate.cargo.build_dir)?;
    std::fs::write(separate.cargo.target_dir.join("other"), b"other owner")?;
    let targets = [fixture.original.clone(), separate.clone()];
    let records = targets
        .iter()
        .flat_map(|roots| evidence(roots, None))
        .collect::<Vec<_>>();
    let records = publish_receipts(&fixture, &records)?;
    let CaptureOutcome::Captured(both) = super::super::super::capture(
        &fixture.config,
        &fixture.store,
        &targets,
        &fixture.config.cache_dir.join(crate::out_dir::ROOT),
        &records,
    )?
    else {
        bail!("independent owners must remain separate");
    };
    std::fs::remove_file(clone.cargo.target_dir.join("artifact"))?;
    std::fs::remove_file(clone.cargo.build_dir.join("fingerprint"))?;
    assert!(matches!(
        super::super::super::restore(
            &fixture.config,
            &fixture.store,
            &both.attachments[ATTACHMENT],
            &clone.workspace_root,
            &clone.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    assert_eq!(
        freeze_lineage(&fixture.config, &fixture.store, &clone)?.map(|proof| proof.owner),
        Some(owner)
    );
    let other = capture(&fixture, &separate, None)?;
    std::fs::remove_file(clone.cargo.target_dir.join("artifact"))?;
    std::fs::remove_file(clone.cargo.build_dir.join("fingerprint"))?;
    assert_eq!(
        super::super::super::restore(
            &fixture.config,
            &fixture.store,
            &other.attachments[ATTACHMENT],
            &clone.workspace_root,
            &clone.cargo
        )?,
        RestoreOutcome::SkippedUnavailable
    );
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn exact_complete_budget_reuses_key_and_one_byte_less_writes_nothing() -> Result<()> {
    let mut fixture = fixture()?;
    let additions = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    super::super::super::restore(
        &fixture.config,
        &fixture.store,
        &additions.attachments[ATTACHMENT],
        &clone.workspace_root,
        &clone.cargo,
    )?;
    let proof = freeze_lineage(&fixture.config, &fixture.store, &clone)?
        .ok_or_else(|| eyre::eyre!("missing native grant"))?;
    let namespace = fixture.store.join("build-receipts/v4/local-grants");
    let key_path = namespace.join("owner-key");
    let grant_name = CacheDigest::blake3(&mbx_cache_core::canonical_json(&clone)?).hash;
    let grant_path = namespace.join(format!("{grant_name}.json"));
    let key = std::fs::read(&key_path)?;
    let grant = std::fs::read(&grant_path)?;
    let required = proof
        .selected_objects
        .iter()
        .map(|object| object.size)
        .sum::<u64>()
        + u64::try_from(grant.len())?
        + 32;
    fixture.config.gc.max_bytes = required;
    local::store(&fixture.config, &proof)?;
    assert_eq!(std::fs::read(&key_path)?, key);
    assert_eq!(std::fs::read(&grant_path)?, grant);
    fixture.config.gc.max_bytes = required - 1;
    assert!(local::store(&fixture.config, &proof).is_err());
    assert_eq!(std::fs::read(&key_path)?, key);
    assert_eq!(std::fs::read(&grant_path)?, grant);
    Ok(())
}

#[test]
fn transient_fabricated_receipt_cannot_create_a_native_snapshot_owner() -> Result<()> {
    let fixture = fixture()?;
    let transient = evidence(&fixture.original, None);
    assert!(mbx_cache_store::stored_receipt_evidence(&fixture.store)?.is_empty());
    let result = super::super::super::capture(
        &fixture.config,
        &fixture.store,
        std::slice::from_ref(&fixture.original),
        &fixture.config.cache_dir.join(crate::out_dir::ROOT),
        &transient,
    )?;
    let CaptureOutcome::UnavailableOwnerProof { reason } = result else {
        bail!("transient receipts must not create portable owner authority");
    };
    assert!(reason.contains("not persisted in its exact canonical registry"));
    assert!(mbx_cache_store::stored_receipt_evidence(&fixture.store)?.is_empty());
    assert!(
        !fixture
            .store
            .join("build-receipts/v4/local-grants")
            .exists()
    );
    Ok(())
}
