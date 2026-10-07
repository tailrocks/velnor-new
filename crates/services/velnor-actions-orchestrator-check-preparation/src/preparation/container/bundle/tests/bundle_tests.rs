use super::*;

#[test]
fn projects_full_tree_and_rejects_overwrite() {
    let (_temp, home, sdk, _) = fixture();
    let projection = project_sdk(&home, &sdk).expect("projection");
    assert_eq!(
        fs::read(&projection.orbctl_program).expect("owned"),
        b"orbctl bytes"
    );
    assert!(
        projection
            .owned_bundle
            .join("Contents/Info.plist")
            .is_file()
    );
    #[cfg(unix)]
    {
        assert_eq!(
            fs::metadata(&projection.orbctl_program)
                .expect("orbctl metadata")
                .permissions()
                .mode()
                & 0o777,
            0o500
        );
        assert_eq!(
            fs::metadata(&projection.owned_bundle)
                .expect("bundle metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    revalidate_sdk(&projection, &sdk).expect("revalidate");
    assert!(matches!(
        project_sdk(&home, &sdk),
        Err(OrchestratorError::OverwriteRefused { .. })
    ));
}

#[cfg(unix)]
#[test]
fn special_modes_are_rejected_in_source_and_owned_tree() {
    let (_temp, home, sdk, cli) = fixture();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o4755)).expect("source root mode");
    assert!(project_sdk(&home, &sdk).is_err());

    let (_temp, home, sdk, cli) = fixture();
    fs::set_permissions(cli.join("Contents"), fs::Permissions::from_mode(0o2755))
        .expect("source child mode");
    assert!(project_sdk(&home, &sdk).is_err());

    for mode in [0o4500, 0o2500] {
        let (_temp, home, sdk, _) = fixture();
        let projection = project_sdk(&home, &sdk).expect("projection");
        fs::set_permissions(&projection.orbctl_program, fs::Permissions::from_mode(mode))
            .expect("owned file mode");
        assert!(revalidate_sdk(&projection, &sdk).is_err());
    }

    for mode in [0o4700, 0o2700] {
        let (_temp, home, sdk, _) = fixture();
        let projection = project_sdk(&home, &sdk).expect("projection");
        fs::set_permissions(&projection.owned_bundle, fs::Permissions::from_mode(mode))
            .expect("owned root mode");
        assert!(revalidate_sdk(&projection, &sdk).is_err());
    }

    let (_temp, home, sdk, _) = fixture();
    let projection = project_sdk(&home, &sdk).expect("projection");
    fs::set_permissions(
        projection.owned_bundle.join("Contents"),
        fs::Permissions::from_mode(0o2700),
    )
    .expect("owned child mode");
    assert!(revalidate_sdk(&projection, &sdk).is_err());
}

#[cfg(unix)]
#[test]
fn owned_tree_revalidation_detects_added_file_and_mode() {
    let (_temp, home, sdk, _) = fixture();
    let projection = project_sdk(&home, &sdk).expect("projection");
    let added = projection.owned_bundle.join("added");
    fs::write(&added, b"unexpected").expect("added");
    assert!(revalidate_sdk(&projection, &sdk).is_err());
    fs::remove_file(added).expect("remove added");
    fs::set_permissions(
        projection.owned_bundle.join("Contents"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("mode");
    assert!(revalidate_sdk(&projection, &sdk).is_err());
}

#[test]
fn outer_hash_is_checked_before_projection() {
    let (_temp, home, mut sdk, _) = fixture();
    sdk.info_plist_sha256 = "0".repeat(64);
    assert!(project_sdk(&home, &sdk).is_err());
    assert!(!home.join("orbstack-sdk").exists());
}

#[cfg(unix)]
#[test]
fn nested_symlinks_are_rejected() {
    let (_temp, home, sdk, cli) = fixture();
    std::os::unix::fs::symlink("orbctl", cli.join("Contents/MacOS/link")).expect("link");
    assert!(project_sdk(&home, &sdk).is_err());
    assert!(!home.join("orbstack-sdk").exists());
}
