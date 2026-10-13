use super::managed_tests::managed_fixture;
use super::*;
use std::fs;

fn fixture(base: &Path, store: &Path, shape: u8) -> Result<(WorkspaceRoots, PathBuf)> {
    let (mut roots, managed) = managed_fixture(base, store)?;
    roots.cargo.build_dir = match shape {
        0 => roots.cargo.target_dir.clone(),
        1 => base.join("build"),
        2 => roots.cargo.target_dir.join("build"),
        _ => {
            let build = base.join("build");
            let target = build.join("chain/target");
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| eyre::eyre!("missing parent"))?,
            )?;
            fs::rename(&roots.cargo.target_dir, &target)?;
            roots.cargo.target_dir = target;
            build
        }
    };
    fs::create_dir_all(&roots.cargo.build_dir)?;
    fs::write(roots.cargo.build_dir.join("scheduler"), b"scheduler bytes")?;
    Ok((roots, managed))
}

fn wipe(roots: &CargoBuildRoots, managed: &Path, shape: u8) -> Result<()> {
    fs::remove_dir_all(managed)?;
    if shape == 1 {
        fs::remove_dir_all(&roots.build_dir)?;
    }
    if shape == 3 {
        fs::remove_file(roots.build_dir.join("scheduler"))?;
    }
    Ok(())
}

#[test]
fn dangling_owned_view_restores_all_role_layouts_with_new_config() -> Result<()> {
    for shape in 0..4 {
        let store = tempfile::tempdir()?;
        let (roots, managed) = fixture(&store.path().join("source"), store.path(), shape)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots))?;
        let attachment = additions
            .attachments
            .get(ATTACHMENT)
            .ok_or_else(|| eyre::eyre!("missing attachment"))?;
        let before = semantic_inventory(store.path(), Some(attachment))?;
        let link = fs::read_link(&roots.cargo.target_dir)?;
        let record = fs::read(managed.with_extension("json"))?;
        wipe(&roots.cargo, &managed, shape)?;
        let mut config = Config::for_test(store.path());
        config.target.root = store.path().join("new-target-root");
        assert!(matches!(
            restore(
                &config,
                store.path(),
                attachment,
                &roots.workspace_root,
                &roots.cargo
            )?,
            RestoreOutcome::Restored { .. }
        ));
        assert_eq!(fs::read_link(&roots.cargo.target_dir)?, link);
        assert_eq!(fs::read(managed.with_extension("json"))?, record);
        assert_eq!(
            fs::read(roots.cargo.target_dir.join("artifact"))?,
            b"owned bytes"
        );
        assert_eq!(
            fs::read(roots.cargo.build_dir.join("scheduler"))?,
            b"scheduler bytes"
        );
        let after = capture(store.path(), std::slice::from_ref(&roots))?;
        assert_eq!(
            before,
            semantic_inventory(store.path(), after.attachments.get(ATTACHMENT))?
        );
    }
    Ok(())
}

#[test]
fn corrupt_second_tree_preserves_owned_link_record_and_both_empty_roots() -> Result<()> {
    for shape in 1..4 {
        let store = tempfile::tempdir()?;
        let (roots, managed) = fixture(&store.path().join("source"), store.path(), shape)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots))?;
        let attachment = additions
            .attachments
            .get(ATTACHMENT)
            .ok_or_else(|| eyre::eyre!("missing attachment"))?;
        let cas = LocalCas::new(store.path());
        let bundle: Bundle = serde_json::from_slice(&fs::read(cas.path_for(attachment)?)?)?;
        fs::write(
            cas.path_for(&bundle.workspaces[0].trees[1].inline_archive)?,
            b"corrupt",
        )?;
        let link = fs::read_link(&roots.cargo.target_dir)?;
        let record = fs::read(managed.with_extension("json"))?;
        wipe(&roots.cargo, &managed, shape)?;
        assert!(
            restore(
                &Config::for_test(store.path()),
                store.path(),
                attachment,
                &roots.workspace_root,
                &roots.cargo
            )
            .is_err()
        );
        assert_eq!(fs::read_link(&roots.cargo.target_dir)?, link);
        assert_eq!(fs::read(managed.with_extension("json"))?, record);
        assert!(!managed.exists());
        assert!(!roots.cargo.build_dir.join("scheduler").exists());
    }
    Ok(())
}

#[test]
fn owner_record_under_build_root_is_preserved_and_foreign_metadata_refused() -> Result<()> {
    let store = tempfile::tempdir()?;
    let workspace_root = store.path().join("workspace");
    fs::create_dir_all(&workspace_root)?;
    fs::write(
        workspace_root.join("Cargo.toml"),
        b"[workspace]\nmembers = []\n",
    )?;
    let build_dir = store.path().join("build");
    let mut config = Config::for_test(store.path());
    config.target.root = build_dir.join("mbx-targets");
    let target_dir = workspace_root.join("target");
    let managed = crate::target::place(&config, &workspace_root, &target_dir, false)
        .ok_or_else(|| eyre::eyre!("managed placement failed"))?;
    fs::write(target_dir.join("artifact"), b"compiled")?;
    fs::write(build_dir.join("scheduler"), b"scheduler")?;
    let roots = WorkspaceRoots {
        workspace_root,
        cargo: CargoBuildRoots {
            target_dir,
            build_dir,
        },
    };
    let additions = capture(store.path(), std::slice::from_ref(&roots))?;
    let attachment = additions
        .attachments
        .get(ATTACHMENT)
        .ok_or_else(|| eyre::eyre!("missing attachment"))?;
    let before = semantic_inventory(store.path(), Some(attachment))?;
    let record = managed.with_extension("json");
    let record_bytes = fs::read(&record)?;
    fs::remove_dir_all(&managed)?;
    fs::remove_file(roots.cargo.build_dir.join("scheduler"))?;
    let foreign = record
        .parent()
        .ok_or_else(|| eyre::eyre!("missing record parent"))?
        .join("unowned.json");
    fs::write(&foreign, b"foreign")?;
    assert_eq!(
        restore(
            &config,
            store.path(),
            attachment,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::SkippedNonempty
    );
    assert!(!managed.exists());
    assert_eq!(fs::read(&record)?, record_bytes);
    fs::remove_file(foreign)?;
    config.target.root = store.path().join("new-target-config");
    let record_metadata = fs::metadata(&record)?;
    assert!(matches!(
        restore(
            &config,
            store.path(),
            attachment,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::Restored { .. }
    ));
    assert_eq!(fs::read(&record)?, record_bytes);
    assert_eq!(
        file_mode(&fs::metadata(&record)?),
        file_mode(&record_metadata)
    );
    assert_eq!(
        modified_parts(&fs::metadata(&record)?),
        modified_parts(&record_metadata)
    );
    let after = capture(store.path(), std::slice::from_ref(&roots))?;
    assert_eq!(
        before,
        semantic_inventory(store.path(), after.attachments.get(ATTACHMENT))?
    );
    Ok(())
}
