use super::*;

#[path = "standalone_lineage_export_tests.rs"]
mod standalone_export_tests;

struct Fixture {
    _directory: tempfile::TempDir,
    config: Config,
    store: PathBuf,
    original: WorkspaceRoots,
}

fn fixture() -> Result<Fixture> {
    let directory = tempfile::tempdir()?;
    let config = Config::for_test(&directory.path().join("cache"));
    let store = config.store_dir();
    std::fs::create_dir_all(&store)?;
    let original = checkout(directory.path(), "original")?;
    std::fs::create_dir_all(&original.cargo.target_dir)?;
    std::fs::create_dir_all(&original.cargo.build_dir)?;
    std::fs::write(
        original.cargo.target_dir.join("artifact"),
        b"complete native artifact",
    )?;
    std::fs::write(
        original.cargo.build_dir.join("fingerprint"),
        b"native scheduler bytes",
    )?;
    Ok(Fixture {
        _directory: directory,
        config,
        store,
        original,
    })
}

fn checkout(root: &Path, name: &str) -> Result<WorkspaceRoots> {
    let workspace_root = root.join(name);
    std::fs::create_dir_all(&workspace_root)?;
    std::fs::write(
        workspace_root.join("Cargo.toml"),
        b"[workspace]\nmembers=[]\n",
    )?;
    std::fs::write(workspace_root.join("Cargo.lock"), b"version=4\n")?;
    Ok(WorkspaceRoots {
        workspace_root,
        cargo: CargoBuildRoots {
            target_dir: root.join(format!("{name}-target")),
            build_dir: root.join(format!("{name}-build")),
        },
    })
}

fn evidence(roots: &WorkspaceRoots, lineage: Option<ReceiptLineage>) -> Vec<ReceiptEvidence> {
    vec![ReceiptEvidence {
        workspace: roots.clone(),
        identity: "a".repeat(64),
        context: None,
        predictions: vec![],
        lineage,
    }]
}

fn capture(
    fixture: &Fixture,
    roots: &WorkspaceRoots,
    proof: Option<ReceiptLineage>,
) -> Result<ExportAdditions> {
    let records = publish_receipts(fixture, &evidence(roots, proof))?;
    match super::super::capture(
        &fixture.config,
        &fixture.store,
        std::slice::from_ref(roots),
        &fixture.config.cache_dir.join(crate::out_dir::ROOT),
        &records,
    )? {
        CaptureOutcome::Captured(additions) | CaptureOutcome::RetainedOwner { additions, .. } => {
            Ok(additions)
        }
        outcome => bail!("fixture capture unavailable: {outcome:?}"),
    }
}

fn publish_receipts(
    fixture: &Fixture,
    records: &[ReceiptEvidence],
) -> Result<Vec<ReceiptEvidence>> {
    super::super::tests::persisted_evidence(&fixture.store, records)
}

fn state(fixture: &Fixture, additions: &ExportAdditions) -> Result<Bundle> {
    read_canonical(
        &LocalCas::new(&fixture.store),
        &additions.attachments[ATTACHMENT],
    )
}

#[cfg(unix)]
#[test]
fn clone_chain_preserves_one_owner_and_original_baseline() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let original_owner = state(&fixture, &initial)?.workspaces[0].owner.clone();
    let second = checkout(fixture._directory.path(), "second")?;
    assert!(matches!(
        super::super::restore(
            &fixture.config,
            &fixture.store,
            &initial.attachments[ATTACHMENT],
            &second.workspace_root,
            &second.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    let frozen = freeze_lineage(&fixture.config, &fixture.store, &second)?
        .ok_or_else(|| eyre::eyre!("missing grant"))?;
    assert_eq!(frozen.origin, fixture.original);
    assert_eq!(frozen.owner, original_owner);
    let original_artifact = fixture.original.cargo.target_dir.join("artifact");
    let restored_artifact = second.cargo.target_dir.join("artifact");
    assert_eq!(
        std::fs::read(&original_artifact)?,
        std::fs::read(&restored_artifact)?
    );
    assert_eq!(
        file_mode(&std::fs::metadata(&original_artifact)?),
        file_mode(&std::fs::metadata(&restored_artifact)?)
    );
    assert_eq!(
        modified_parts(&std::fs::metadata(&original_artifact)?),
        modified_parts(&std::fs::metadata(&restored_artifact)?)
    );
    let fixed_closure = frozen.selected_objects.clone();
    let current = capture(&fixture, &second, Some(frozen))?;
    let retained = super::super::retain(
        &fixture.store,
        current,
        Some(&initial.attachments[ATTACHMENT]),
    )?;
    let bundle = state(&fixture, &retained.additions)?;
    assert_eq!(bundle.workspaces.len(), 1);
    assert_eq!(bundle.workspaces[0].owner, original_owner);
    assert_eq!(
        bundle.workspaces[0].workspace_root,
        fixture.original.workspace_root
    );
    let third = checkout(fixture._directory.path(), "third")?;
    assert!(matches!(
        super::super::restore(
            &fixture.config,
            &fixture.store,
            &retained.additions.attachments[ATTACHMENT],
            &third.workspace_root,
            &third.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    let final_proof = freeze_lineage(&fixture.config, &fixture.store, &third)?
        .ok_or_else(|| eyre::eyre!("missing third native grant"))?;
    assert_eq!(final_proof.owner, original_owner);
    assert_eq!(final_proof.selected_objects, fixed_closure);
    Ok(())
}

#[cfg(unix)]
#[test]
fn post_build_changes_keep_lineage_but_pruning_cannot_replace_baseline() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    super::super::restore(
        &fixture.config,
        &fixture.store,
        &initial.attachments[ATTACHMENT],
        &clone.workspace_root,
        &clone.cargo,
    )?;
    let before = freeze_lineage(&fixture.config, &fixture.store, &clone)?;
    std::fs::remove_file(clone.cargo.target_dir.join("artifact"))?;
    assert_eq!(
        freeze_lineage(&fixture.config, &fixture.store, &clone)?,
        before
    );
    let records = publish_receipts(&fixture, &evidence(&clone, before))?;
    let outcome = super::super::capture(
        &fixture.config,
        &fixture.store,
        std::slice::from_ref(&clone),
        &fixture.config.cache_dir.join(crate::out_dir::ROOT),
        &records,
    )?;
    let CaptureOutcome::RetainedOwner {
        additions: current,
        reasons,
    } = outcome
    else {
        bail!("pruned native payload must retain the selected owner explicitly");
    };
    assert!(!reasons.is_empty());
    let retained = super::super::retain(
        &fixture.store,
        current,
        Some(&initial.attachments[ATTACHMENT]),
    )?;
    assert_eq!(
        retained.additions.attachments[ATTACHMENT],
        initial.attachments[ATTACHMENT]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn nonempty_restore_revokes_grant_and_receipt_snapshot_stays_frozen() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    super::super::restore(
        &fixture.config,
        &fixture.store,
        &initial.attachments[ATTACHMENT],
        &clone.workspace_root,
        &clone.cargo,
    )?;
    let frozen = freeze_lineage(&fixture.config, &fixture.store, &clone)?
        .ok_or_else(|| eyre::eyre!("missing grant"))?;
    let receipt = publish_receipts(&fixture, &evidence(&clone, Some(frozen.clone())))?;
    assert_eq!(
        super::super::restore(
            &fixture.config,
            &fixture.store,
            &initial.attachments[ATTACHMENT],
            &clone.workspace_root,
            &clone.cargo
        )?,
        RestoreOutcome::SkippedNonempty
    );
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone)?.is_none());
    assert_eq!(receipt[0].lineage.as_ref(), Some(&frozen));
    assert!(frozen_owner(&fixture.config, &fixture.store, &clone, &receipt).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn canonical_self_asserted_json_cannot_grant_lineage() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    super::super::restore(
        &fixture.config,
        &fixture.store,
        &initial.attachments[ATTACHMENT],
        &clone.workspace_root,
        &clone.cargo,
    )?;
    let roots_digest = CacheDigest::blake3(&mbx_cache_core::canonical_json(&clone)?);
    let path = fixture
        .store
        .join("build-receipts/v4/local-grants")
        .join(format!("{}.json", roots_digest.hash));
    let mut grant: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    grant["grant"]["proof"]["selected_state"] =
        serde_json::to_value(CacheDigest::blake3(b"forged"))?;
    std::fs::write(path, mbx_cache_core::canonical_json(&grant)?)?;
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn recreated_root_and_missing_selected_object_refuse_lineage() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    super::super::restore(
        &fixture.config,
        &fixture.store,
        &initial.attachments[ATTACHMENT],
        &clone.workspace_root,
        &clone.cargo,
    )?;
    std::fs::rename(
        &clone.cargo.target_dir,
        clone.cargo.target_dir.with_extension("old"),
    )?;
    std::fs::create_dir(&clone.cargo.target_dir)?;
    assert!(freeze_lineage(&fixture.config, &fixture.store, &clone).is_err());
    let next = checkout(fixture._directory.path(), "next")?;
    super::super::restore(
        &fixture.config,
        &fixture.store,
        &initial.attachments[ATTACHMENT],
        &next.workspace_root,
        &next.cargo,
    )?;
    let cas = LocalCas::new(&fixture.store);
    std::fs::remove_file(cas.path_for(&initial.attachments[ATTACHMENT])?)?;
    assert!(freeze_lineage(&fixture.config, &fixture.store, &next).is_err());
    Ok(())
}

#[test]
fn unbound_compatible_clone_cannot_append_to_baseline() -> Result<()> {
    let fixture = fixture()?;
    let initial = capture(&fixture, &fixture.original, None)?;
    let clone = checkout(fixture._directory.path(), "clone")?;
    std::fs::create_dir_all(&clone.cargo.target_dir)?;
    std::fs::create_dir_all(&clone.cargo.build_dir)?;
    std::fs::write(
        clone.cargo.target_dir.join("different"),
        b"independent bytes",
    )?;
    let current = capture(&fixture, &clone, None)?;
    assert_ne!(
        state(&fixture, &initial)?.workspaces[0].owner,
        state(&fixture, &current)?.workspaces[0].owner
    );
    let retained = super::super::retain(
        &fixture.store,
        current,
        Some(&initial.attachments[ATTACHMENT]),
    )?;
    assert!(!retained.unavailable_reasons.is_empty());
    assert_eq!(
        retained.additions.attachments[ATTACHMENT],
        initial.attachments[ATTACHMENT]
    );
    Ok(())
}

#[path = "lineage_negative_tests.rs"]
mod negatives;
