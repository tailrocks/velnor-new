use super::role_tests::{clear_roots, digest, roots_fixture};
use super::*;
use std::fs;

#[test]
fn nested_empty_skeletons_restore_both_role_orders() -> Result<()> {
    for shape in [2, 3] {
        let store = tempfile::tempdir()?;
        let roots = roots_fixture(&store.path().join("source"), shape)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots))?;
        clear_roots(&roots.cargo)?;
        fs::create_dir_all(&roots.cargo.target_dir)?;
        fs::create_dir_all(&roots.cargo.build_dir)?;
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
        assert!(roots.cargo.target_dir.join("artifact").is_file());
        assert!(roots.cargo.build_dir.join("scheduler").is_file());
    }
    Ok(())
}

#[test]
fn nested_foreign_empty_siblings_refuse_and_corrupt_state_preserves_skeleton() -> Result<()> {
    for shape in [2, 3] {
        let store = tempfile::tempdir()?;
        let roots = roots_fixture(&store.path().join("source"), shape)?;
        let additions = capture(store.path(), std::slice::from_ref(&roots))?;
        clear_roots(&roots.cargo)?;
        fs::create_dir_all(&roots.cargo.target_dir)?;
        fs::create_dir_all(&roots.cargo.build_dir)?;
        let outer = if shape == 2 {
            &roots.cargo.target_dir
        } else {
            &roots.cargo.build_dir
        };
        let foreign = outer.join("foreign");
        fs::create_dir(&foreign)?;
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
        assert!(foreign.is_dir());
        fs::remove_dir(foreign)?;
        let cas = LocalCas::new(store.path());
        let bundle: Bundle =
            serde_json::from_slice(&fs::read(cas.path_for(digest(&additions)?)?)?)?;
        fs::write(
            cas.path_for(&bundle.workspaces[0].trees[1].inline_archive)?,
            b"corrupt",
        )?;
        let before_target = fs::metadata(&roots.cargo.target_dir)?;
        let before_build = fs::metadata(&roots.cargo.build_dir)?;
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
        assert_eq!(
            before_target.modified()?,
            fs::metadata(&roots.cargo.target_dir)?.modified()?
        );
        assert_eq!(
            before_build.modified()?,
            fs::metadata(&roots.cargo.build_dir)?.modified()?
        );
        assert!(!roots.cargo.target_dir.join("artifact").exists());
        assert!(!roots.cargo.build_dir.join("scheduler").exists());
    }
    Ok(())
}

#[test]
fn publication_failure_restores_original_nested_skeleton_metadata() -> Result<()> {
    let root = tempfile::tempdir()?;
    let target_dir = root.path().join("target");
    let build_dir = target_dir.join("chain/build");
    fs::create_dir_all(&build_dir)?;
    let original_time = filetime::FileTime::from_unix_time(123, 456);
    filetime::set_file_mtime(&target_dir, original_time)?;
    filetime::set_file_mtime(&build_dir, original_time)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o750))?;
        fs::set_permissions(&build_dir, fs::Permissions::from_mode(0o700))?;
    }
    let target_mode = file_mode(&fs::metadata(&target_dir)?);
    let build_mode = file_mode(&fs::metadata(&build_dir)?);
    let staging = tempfile::tempdir_in(root.path())?;
    let missing_stage = staging.path().join("missing");
    let publications = vec![(target_dir.clone(), staging, missing_stage)];
    let roots = CargoBuildRoots {
        target_dir: target_dir.clone(),
        build_dir: build_dir.clone(),
    };
    assert!(super::restore::publish_roots(&publications, &roots, None).is_err());
    for (path, mode) in [(&target_dir, target_mode), (&build_dir, build_mode)] {
        let metadata = fs::metadata(path)?;
        assert_eq!(file_mode(&metadata), mode);
        assert_eq!(
            filetime::FileTime::from_last_modification_time(&metadata),
            original_time
        );
    }
    assert!(fs::read_dir(build_dir)?.next().is_none());
    Ok(())
}
