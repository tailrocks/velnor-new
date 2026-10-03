use super::role_tests::{clear_roots, digest, roots_fixture};
use super::*;

fn capture(
    store: &Path,
    targets: &[WorkspaceRoots],
    owner: &Path,
    _: &[mbx_cache_store::ReceiptEvidence],
) -> Result<CaptureOutcome> {
    let evidence =
        super::tests::persisted_evidence(store, &super::tests::fixture_evidence(targets))?;
    super::capture(&Config::for_test(store), store, targets, owner, &evidence)
}

#[test]
fn common_cache_ancestor_with_disjoint_owned_namespaces_is_supported() -> Result<()> {
    let base = tempfile::tempdir()?;
    let cache = base.path().join("cache");
    let store = cache.join("actions");
    std::fs::create_dir_all(&store)?;
    let roots = roots_fixture(&cache, 1)?;
    assert!(matches!(
        capture(
            &store,
            std::slice::from_ref(&roots),
            &cache.join(crate::out_dir::ROOT),
            &[]
        )?,
        CaptureOutcome::Captured(_)
    ));
    Ok(())
}

#[test]
fn native_store_and_owner_overlap_are_unavailable_before_capture_mutation() -> Result<()> {
    for kind in 0..3 {
        let base = tempfile::tempdir()?;
        let mut roots = roots_fixture(&base.path().join("source"), 1)?;
        let store = if kind == 0 {
            roots.cargo.build_dir.join("native/actions")
        } else {
            base.path().join("actions")
        };
        std::fs::create_dir_all(&store)?;
        std::fs::write(store.join("sentinel"), b"preserve")?;
        let owner = if kind == 1 {
            roots.cargo.build_dir.join("native/out-dirs/v1")
        } else {
            base.path().join("out-dirs/v1")
        };
        if kind == 2 {
            roots.cargo.build_dir = store.join("cas/v1/cargo-build");
            std::fs::create_dir_all(&roots.cargo.build_dir)?;
        }
        assert!(matches!(
            super::capture(
                &Config::for_test(&store),
                &store,
                std::slice::from_ref(&roots),
                &owner,
                &[]
            )?,
            CaptureOutcome::UnavailableManagedOverlap
        ));
        assert_eq!(std::fs::read(store.join("sentinel"))?, b"preserve");
        assert!(!owner.exists());
        if kind != 2 {
            assert_eq!(std::fs::read_dir(&store)?.count(), 1);
        }
    }
    Ok(())
}

#[test]
fn group_placement_preflight_precedes_capture_of_every_valid_pair() -> Result<()> {
    let base = tempfile::tempdir()?;
    let store = base.path().join("actions");
    std::fs::create_dir_all(&store)?;
    let first = roots_fixture(&base.path().join("first"), 1)?;
    let mut second = roots_fixture(&base.path().join("second"), 1)?;
    second.cargo.build_dir = base.path().to_path_buf();
    assert!(matches!(
        super::capture(
            &Config::for_test(&store),
            &store,
            &[first, second],
            &base.path().join("out-dirs/v1"),
            &[]
        )?,
        CaptureOutcome::UnavailableManagedOverlap
    ));
    assert_eq!(std::fs::read_dir(&store)?.count(), 0);
    Ok(())
}

#[test]
fn restore_owner_overlap_skips_before_cargo_or_owner_publication() -> Result<()> {
    let base = tempfile::tempdir()?;
    let store = base.path().join("actions");
    std::fs::create_dir_all(&store)?;
    let roots = roots_fixture(&base.path().join("source"), 1)?;
    let CaptureOutcome::Captured(additions) = capture(
        &store,
        std::slice::from_ref(&roots),
        &base.path().join("out-dirs/v1"),
        &[],
    )?
    else {
        bail!("fixture capture unavailable")
    };
    clear_roots(&roots.cargo)?;
    let config = Config::for_test(&roots.cargo.build_dir.join("native"));
    assert_eq!(
        restore(
            &config,
            &store,
            digest(&additions)?,
            &roots.workspace_root,
            &roots.cargo
        )?,
        RestoreOutcome::SkippedManagedOverlap
    );
    assert!(!roots.cargo.target_dir.exists());
    assert!(!roots.cargo.build_dir.exists());
    assert!(!config.cache_dir.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn physical_owner_alias_into_build_is_also_unavailable() -> Result<()> {
    let base = tempfile::tempdir()?;
    let store = base.path().join("actions");
    std::fs::create_dir_all(&store)?;
    let roots = roots_fixture(&base.path().join("source"), 1)?;
    let real_cache = roots.cargo.build_dir.join("native");
    std::fs::create_dir_all(real_cache.join(crate::out_dir::ROOT))?;
    let alias = base.path().join("alias");
    std::os::unix::fs::symlink(&real_cache, &alias)?;
    assert!(matches!(
        super::capture(
            &Config::for_test(&store),
            &store,
            std::slice::from_ref(&roots),
            &alias.join(crate::out_dir::ROOT),
            &[]
        )?,
        CaptureOutcome::UnavailableManagedOverlap
    ));
    assert_eq!(std::fs::read_dir(&store)?.count(), 0);
    Ok(())
}
