use super::*;
use std::fs;

pub(super) fn roots_fixture(base: &Path, shape: u8) -> Result<WorkspaceRoots> {
    let workspace_root = base.join("workspace");
    fs::create_dir_all(&workspace_root)?;
    fs::write(
        workspace_root.join("Cargo.toml"),
        b"[workspace]\nmembers = []\n",
    )?;
    let target_dir = base.join("target");
    let cargo = match shape {
        0 => CargoBuildRoots {
            build_dir: target_dir.clone(),
            target_dir,
        },
        1 => CargoBuildRoots {
            target_dir,
            build_dir: base.join("build"),
        },
        2 => CargoBuildRoots {
            build_dir: target_dir.join("build"),
            target_dir,
        },
        _ => CargoBuildRoots {
            target_dir: base.join("build/target"),
            build_dir: base.join("build"),
        },
    };
    fs::create_dir_all(&cargo.target_dir)?;
    fs::create_dir_all(&cargo.build_dir)?;
    fs::write(cargo.target_dir.join("artifact"), b"final bytes")?;
    fs::write(cargo.build_dir.join("scheduler"), b"scheduler bytes")?;
    fs::write(cargo.build_dir.join(".rustc_info.json"), b"probe bytes")?;
    Ok(WorkspaceRoots {
        workspace_root,
        cargo,
    })
}

pub(super) fn digest(additions: &ExportAdditions) -> Result<&CacheDigest> {
    additions
        .attachments
        .get(ATTACHMENT)
        .ok_or_else(|| eyre::eyre!("missing fixture attachment"))
}

pub(super) fn clear_roots(roots: &CargoBuildRoots) -> Result<()> {
    if roots.build_dir.starts_with(&roots.target_dir) {
        fs::remove_dir_all(&roots.target_dir)?;
    } else if roots.target_dir.starts_with(&roots.build_dir) {
        fs::remove_dir_all(&roots.build_dir)?;
    } else {
        fs::remove_dir_all(&roots.target_dir)?;
        fs::remove_dir_all(&roots.build_dir)?;
    }
    Ok(())
}

#[test]
fn all_root_shapes_roundtrip_without_duplicate_nested_payloads() -> Result<()> {
    for shape in 0..4 {
        let store = tempfile::tempdir()?;
        let roots = roots_fixture(&store.path().join("source"), shape)?;
        filetime::set_file_mtime(
            roots.cargo.build_dir.join("scheduler"),
            filetime::FileTime::from_unix_time(123, 456),
        )?;
        let additions = capture(store.path(), std::slice::from_ref(&roots))?;
        let cas = LocalCas::new(store.path());
        let bundle: Bundle = serde_json::from_slice(&fs::read(
            cas.find(digest(&additions)?)?
                .ok_or_else(|| eyre::eyre!("missing bundle"))?,
        )?)?;
        assert_eq!(
            bundle.workspaces[0].trees.len(),
            if shape == 0 { 1 } else { 2 }
        );
        let before = semantic_inventory(store.path(), Some(digest(&additions)?))?;
        validate_semantic_inventory(&before)?;
        clear_roots(&roots.cargo)?;
        let result = restore(
            &Config::for_test(store.path()),
            store.path(),
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo,
        )?;
        assert!(matches!(result, RestoreOutcome::Restored { .. }));
        assert_eq!(
            fs::read(roots.cargo.target_dir.join("artifact"))?,
            b"final bytes"
        );
        assert_eq!(
            fs::read(roots.cargo.build_dir.join("scheduler"))?,
            b"scheduler bytes"
        );
        assert_eq!(
            fs::read(roots.cargo.build_dir.join(".rustc_info.json"))?,
            b"probe bytes"
        );
        let time = filetime::FileTime::from_last_modification_time(&fs::metadata(
            roots.cargo.build_dir.join("scheduler"),
        )?);
        assert_eq!(time, filetime::FileTime::from_unix_time(123, 456));
        let after = capture(store.path(), std::slice::from_ref(&roots))?;
        assert_eq!(
            before,
            semantic_inventory(store.path(), Some(digest(&after)?))?
        );
    }
    Ok(())
}

#[test]
fn compiler_probe_exclusion_follows_effective_build_role() -> Result<()> {
    let store = tempfile::tempdir()?;
    let roots = roots_fixture(&store.path().join("source"), 1)?;
    fs::write(
        roots.cargo.target_dir.join(".rustc_info.json"),
        b"target data",
    )?;
    let first = capture(store.path(), std::slice::from_ref(&roots))?;
    let before = semantic_inventory(store.path(), Some(digest(&first)?))?;
    fs::write(
        roots.cargo.build_dir.join(".rustc_info.json"),
        b"changed probe",
    )?;
    let second = capture(store.path(), std::slice::from_ref(&roots))?;
    assert_eq!(
        before,
        semantic_inventory(store.path(), Some(digest(&second)?))?
    );
    fs::write(
        roots.cargo.target_dir.join(".rustc_info.json"),
        b"changed target data",
    )?;
    let third = capture(store.path(), std::slice::from_ref(&roots))?;
    assert_ne!(
        before,
        semantic_inventory(store.path(), Some(digest(&third)?))?
    );
    Ok(())
}

#[test]
fn corrupted_second_tree_never_publishes_first_root() -> Result<()> {
    let store = tempfile::tempdir()?;
    let roots = roots_fixture(&store.path().join("source"), 1)?;
    let additions = capture(store.path(), std::slice::from_ref(&roots))?;
    let cas = LocalCas::new(store.path());
    let bundle: Bundle = serde_json::from_slice(&fs::read(cas.path_for(digest(&additions)?)?)?)?;
    fs::write(
        cas.path_for(&bundle.workspaces[0].trees[1].inline_archive)?,
        b"corrupt",
    )?;
    clear_roots(&roots.cargo)?;
    assert!(
        restore(
            &Config::for_test(store.path()),
            store.path(),
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )
        .is_err()
    );
    assert!(!roots.cargo.target_dir.exists());
    assert!(!roots.cargo.build_dir.exists());
    Ok(())
}

#[test]
fn retention_keeps_root_pairs_and_ambiguity_is_explicit() -> Result<()> {
    let store = tempfile::tempdir()?;
    let first = roots_fixture(&store.path().join("first"), 1)?;
    let mut second = roots_fixture(&store.path().join("second"), 1)?;
    second.workspace_root = first.workspace_root.clone();
    let baseline = capture(store.path(), std::slice::from_ref(&first))?;
    let current = capture(store.path(), std::slice::from_ref(&second))?;
    let retained = retain(store.path(), current, Some(digest(&baseline)?))?;
    let cas = LocalCas::new(store.path());
    let bundle: Bundle = serde_json::from_slice(&fs::read(cas.path_for(digest(&retained)?)?)?)?;
    assert_eq!(bundle.workspaces.len(), 2);
    let relocated = roots_fixture(&store.path().join("relocated"), 1)?;
    clear_roots(&relocated.cargo)?;
    assert_eq!(
        restore(
            &Config::for_test(store.path()),
            store.path(),
            digest(&retained)?,
            &relocated.workspace_root,
            &relocated.cargo
        )?,
        RestoreOutcome::SkippedAmbiguous
    );
    clear_roots(&second.cargo)?;
    assert!(matches!(
        restore(
            &Config::for_test(store.path()),
            store.path(),
            digest(&retained)?,
            &second.workspace_root,
            &second.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    Ok(())
}

#[test]
fn nonempty_either_role_blocks_both_roots() -> Result<()> {
    let store = tempfile::tempdir()?;
    let roots = roots_fixture(&store.path().join("source"), 1)?;
    let additions = capture(store.path(), std::slice::from_ref(&roots))?;
    fs::remove_dir_all(&roots.cargo.target_dir)?;
    assert_eq!(
        restore(
            &Config::for_test(store.path()),
            store.path(),
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::SkippedNonempty
    );
    assert!(!roots.cargo.target_dir.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_role_alias_is_rejected() -> Result<()> {
    let store = tempfile::tempdir()?;
    let roots = roots_fixture(&store.path().join("source"), 1)?;
    fs::remove_dir_all(&roots.cargo.build_dir)?;
    std::os::unix::fs::symlink(&roots.cargo.target_dir, &roots.cargo.build_dir)?;
    assert!(capture(store.path(), std::slice::from_ref(&roots)).is_err());
    Ok(())
}

#[test]
fn publication_failure_rolls_back_prior_root_and_preserves_empty_original() -> Result<()> {
    let root = tempfile::tempdir()?;
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::create_dir(&first)?;
    fs::create_dir(&second)?;
    fs::write(second.join("late-change"), b"untouched")?;
    let first_stage = tempfile::tempdir_in(root.path())?;
    let first_payload = first_stage.path().join("root");
    fs::create_dir(&first_payload)?;
    fs::write(first_payload.join("payload"), b"payload")?;
    let second_stage = tempfile::tempdir_in(root.path())?;
    let second_payload = second_stage.path().join("root");
    fs::create_dir(&second_payload)?;
    let publications = vec![
        (first.clone(), first_stage, first_payload),
        (second.clone(), second_stage, second_payload),
    ];
    assert!(
        super::restore::publish_roots(
            &publications,
            &CargoBuildRoots {
                target_dir: first.clone(),
                build_dir: second.clone()
            },
            None
        )
        .is_err()
    );
    assert!(first.is_dir());
    assert!(fs::read_dir(first)?.next().is_none());
    assert_eq!(fs::read(second.join("late-change"))?, b"untouched");
    Ok(())
}

#[test]
fn missing_role_is_an_explicit_empty_tree_and_preserves_existing_role() -> Result<()> {
    for missing_target in [true, false] {
        let store = tempfile::tempdir()?;
        let roots = roots_fixture(&store.path().join("source"), 1)?;
        let missing = if missing_target {
            &roots.cargo.target_dir
        } else {
            &roots.cargo.build_dir
        };
        fs::remove_dir_all(missing)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots))?;
        let cas = LocalCas::new(store.path());
        let bundle: Bundle =
            serde_json::from_slice(&fs::read(cas.path_for(digest(&additions)?)?)?)?;
        assert_eq!(bundle.workspaces[0].trees.len(), 2);
        let existing = if missing_target {
            &roots.cargo.build_dir
        } else {
            &roots.cargo.target_dir
        };
        fs::remove_dir_all(existing)?;
        assert!(matches!(
            restore(
                &Config::for_test(store.path()),
                store.path(),
                digest(&additions)?,
                &roots.workspace_root,
                &roots.cargo
            )?,
            RestoreOutcome::Restored { .. }
        ));
        assert!(missing.is_dir());
        assert!(fs::read_dir(missing)?.next().is_none());
        let file = if missing_target {
            "scheduler"
        } else {
            "artifact"
        };
        assert!(existing.join(file).is_file());
    }
    Ok(())
}

#[test]
fn an_added_unit_replaces_current_pair_wholesale_without_reviving_deleted_entries() -> Result<()> {
    let store = tempfile::tempdir()?;
    let roots = roots_fixture(&store.path().join("source"), 1)?;
    let baseline = capture(store.path(), std::slice::from_ref(&roots))?;
    fs::remove_file(roots.cargo.target_dir.join("artifact"))?;
    fs::write(
        roots.cargo.target_dir.join("new-unit"),
        b"new compiled unit",
    )?;
    let current = capture(store.path(), std::slice::from_ref(&roots))?;
    let retained = retain(store.path(), current, Some(digest(&baseline)?))?;
    let inventory = semantic_inventory(store.path(), Some(digest(&retained)?))?;
    assert!(
        inventory
            .keys()
            .any(|key| key.ends_with("/target/new-unit"))
    );
    assert!(
        !inventory
            .keys()
            .any(|key| key.ends_with("/target/artifact"))
    );
    Ok(())
}

#[test]
fn exact_pair_requires_current_manifest_and_lock_signature() -> Result<()> {
    for name in ["Cargo.toml", "Cargo.lock"] {
        let store = tempfile::tempdir()?;
        let roots = roots_fixture(&store.path().join("source"), 1)?;
        let baseline = capture(store.path(), std::slice::from_ref(&roots))?;
        clear_roots(&roots.cargo)?;
        fs::write(
            roots.workspace_root.join(name),
            b"changed current Cargo source identity",
        )?;
        assert_eq!(
            restore(
                &Config::for_test(store.path()),
                store.path(),
                digest(&baseline)?,
                &roots.workspace_root,
                &roots.cargo
            )?,
            RestoreOutcome::SkippedIncompatible
        );
        assert!(!roots.cargo.target_dir.exists());
        assert!(!roots.cargo.build_dir.exists());
        fs::create_dir_all(&roots.cargo.target_dir)?;
        fs::create_dir_all(&roots.cargo.build_dir)?;
        fs::write(
            roots.cargo.target_dir.join("current-unit"),
            b"current source unit",
        )?;
        let current = capture(store.path(), std::slice::from_ref(&roots))?;
        clear_roots(&roots.cargo)?;
        assert!(matches!(
            restore(
                &Config::for_test(store.path()),
                store.path(),
                digest(&current)?,
                &roots.workspace_root,
                &roots.cargo
            )?,
            RestoreOutcome::Restored { .. }
        ));
        assert_eq!(
            fs::read(roots.cargo.target_dir.join("current-unit"))?,
            b"current source unit"
        );
    }
    Ok(())
}
