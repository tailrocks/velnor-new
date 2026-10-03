use super::tests::capture_fixture as capture;
use super::*;
use std::fs;

fn digest(additions: &ExportAdditions) -> Result<&CacheDigest> {
    additions
        .attachments
        .get(ATTACHMENT)
        .ok_or_else(|| eyre::eyre!("missing fixture attachment"))
}

#[cfg(unix)]
pub(super) fn managed_fixture(base: &Path, store: &Path) -> Result<(WorkspaceRoots, PathBuf)> {
    let workspace_root = base.join("workspace");
    fs::create_dir_all(&workspace_root)?;
    fs::write(
        workspace_root.join("Cargo.toml"),
        b"[workspace]\nmembers = []\n",
    )?;
    let target_dir = workspace_root.join("target");
    let managed = crate::target::place(
        &Config::for_test(store),
        &workspace_root,
        &target_dir,
        false,
    )
    .ok_or_else(|| eyre::eyre!("managed fixture placement failed"))?;
    fs::write(target_dir.join("artifact"), b"owned bytes")?;
    Ok((
        WorkspaceRoots {
            workspace_root,
            cargo: CargoBuildRoots {
                build_dir: target_dir.clone(),
                target_dir,
            },
        },
        managed,
    ))
}

#[cfg(unix)]
#[test]
fn owned_managed_target_capture_preserves_lexical_pair() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (roots, managed) = managed_fixture(&store.path().join("source"), store.path())?;
    let additions = capture(store.path(), std::slice::from_ref(&roots))?;
    let cas = LocalCas::new(store.path());
    let bundle: Bundle = serde_json::from_slice(&fs::read(cas.path_for(digest(&additions)?)?)?)?;
    assert_eq!(bundle.workspaces[0].cargo_roots, roots.cargo);
    assert_ne!(bundle.workspaces[0].cargo_roots.target_dir, managed);
    let inventory = semantic_inventory(store.path(), Some(digest(&additions)?))?;
    assert!(
        inventory
            .keys()
            .any(|path| path.ends_with("/target/artifact"))
    );
    // An owner-proven nonempty view remains untouched.
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
    Ok(())
}

#[cfg(unix)]
#[test]
fn managed_capture_rejects_wrong_record_workspace_escape_and_unrecorded_links() -> Result<()> {
    for failure in 0..4 {
        let store = tempfile::tempdir()?;
        let (mut roots, managed) = managed_fixture(&store.path().join("source"), store.path())?;
        match failure {
            0 => fs::write(managed.with_extension("json"), b"invalid owner record")?,
            1 => {
                roots.workspace_root = store.path().join("different-workspace");
                fs::create_dir_all(&roots.workspace_root)?;
                fs::write(
                    roots.workspace_root.join("Cargo.toml"),
                    b"[workspace]\nmembers = []\n",
                )?;
            }
            2 => {
                fs::remove_dir_all(&managed)?;
                let outside = store.path().join("outside");
                fs::create_dir_all(&outside)?;
                std::os::unix::fs::symlink(outside, &managed)?;
            }
            _ => fs::remove_file(managed.with_extension("json"))?,
        }
        assert!(capture(store.path(), std::slice::from_ref(&roots)).is_err());
    }
    Ok(())
}

#[test]
fn managed_capture_covers_all_role_layouts_and_missing_valid_roles() -> Result<()> {
    for shape in 0..4 {
        for absent in 0..3 {
            // A missing outer root necessarily makes its nested role absent too.
            let store = tempfile::tempdir()?;
            let (mut roots, managed) = managed_fixture(&store.path().join("source"), store.path())?;
            roots.cargo.build_dir = match shape {
                0 => roots.cargo.target_dir.clone(),
                1 => store.path().join("build"),
                2 => roots.cargo.target_dir.join("build"),
                _ => roots.workspace_root.clone(),
            };
            fs::create_dir_all(&roots.cargo.build_dir)?;
            fs::write(roots.cargo.build_dir.join("scheduler"), b"scheduler")?;
            if absent == 1 {
                fs::remove_dir_all(&managed)?;
            } else if absent == 2 {
                if shape == 3 {
                    continue;
                } // The workspace is required to identify any capture.
                fs::remove_dir_all(&roots.cargo.build_dir)?;
            }
            let additions = capture(store.path(), std::slice::from_ref(&roots))?;
            let digest = additions
                .attachments
                .get(ATTACHMENT)
                .ok_or_else(|| eyre::eyre!("missing managed attachment"))?;
            let before = semantic_inventory(store.path(), Some(digest))?;
            validate_semantic_inventory(&before)?;
            let target_dir = store.path().join("destination/target");
            let current = match shape {
                0 => CargoBuildRoots {
                    build_dir: target_dir.clone(),
                    target_dir,
                },
                1 => CargoBuildRoots {
                    target_dir,
                    build_dir: store.path().join("destination/build"),
                },
                2 => CargoBuildRoots {
                    build_dir: target_dir.join("build"),
                    target_dir,
                },
                _ => CargoBuildRoots {
                    target_dir: store.path().join("destination/build/target"),
                    build_dir: store.path().join("destination/build"),
                },
            };
            assert!(matches!(
                restore(
                    &Config::for_test(store.path()),
                    store.path(),
                    digest,
                    &roots.workspace_root,
                    &current
                )?,
                RestoreOutcome::Restored { .. }
            ));
            let restored = WorkspaceRoots {
                workspace_root: roots.workspace_root.clone(),
                cargo: current,
            };
            let after = capture(store.path(), std::slice::from_ref(&restored))?;
            assert_eq!(
                before,
                semantic_inventory(store.path(), after.attachments.get(ATTACHMENT))?
            );
        }
    }
    Ok(())
}

#[test]
fn owned_paths_must_be_normalized_before_capture_or_restore() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (mut roots, _) = managed_fixture(&store.path().join("source"), store.path())?;
    let additions = capture(store.path(), std::slice::from_ref(&roots))?;
    roots.cargo.target_dir = roots.workspace_root.join("./target");
    roots.cargo.build_dir = roots.cargo.target_dir.clone();
    assert!(capture(store.path(), std::slice::from_ref(&roots)).is_err());
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
    Ok(())
}

#[test]
fn owned_target_missing_nested_build_is_empty_but_other_nested_links_are_refused() -> Result<()> {
    let store = tempfile::tempdir()?;
    let (mut roots, managed) = managed_fixture(&store.path().join("source"), store.path())?;
    roots.cargo.build_dir = roots.cargo.target_dir.join("nested/build");
    let additions = capture(store.path(), std::slice::from_ref(&roots))?;
    let cas = LocalCas::new(store.path());
    let bundle: Bundle = serde_json::from_slice(&fs::read(cas.path_for(digest(&additions)?)?)?)?;
    assert!(bundle.workspaces[0].trees[1].inline_files.is_empty());
    let outside = store.path().join("outside");
    fs::create_dir_all(outside.join("build"))?;
    std::os::unix::fs::symlink(&outside, managed.join("nested"))?;
    assert!(capture(store.path(), std::slice::from_ref(&roots)).is_err());
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
    Ok(())
}
