use super::tree;
use super::*;
use crate::cover_identity::generator::sha256_hex;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tempfile::TempDir;
use velnor_actions_contract::config::HostOrbStackSdk;

fn fixture() -> (TempDir, PathBuf, HostOrbStackSdk, PathBuf) {
    let temp = tempfile::tempdir().expect("temp");
    let home = temp.path().canonicalize().expect("canonical temp");
    let app = home.join("OrbStack.app");
    let cli = app.join("Contents/Resources/orbstack-cli.app");
    fs::create_dir_all(cli.join("Contents/MacOS")).expect("dirs");
    fs::write(app.join("Contents/Info.plist"), b"outer plist").expect("plist");
    fs::create_dir_all(app.join("Contents/MacOS")).expect("main dir");
    fs::write(app.join("Contents/MacOS/OrbStack"), b"outer executable").expect("main");
    fs::write(cli.join("Contents/Info.plist"), b"cli plist").expect("cli plist");
    fs::write(cli.join("Contents/MacOS/orbctl"), b"orbctl bytes").expect("orbctl");
    #[cfg(unix)]
    {
        fs::set_permissions(
            cli.join("Contents/MacOS/orbctl"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("exec");
    }
    let source_entries = tree::collect_tree(&cli).expect("source tree");
    let source_hash = tree::tree_digest(&source_entries).expect("source digest");
    let expected = home.join("expected");
    tree::create_destination(&expected).expect("expected dir");
    tree::populate(&expected, &source_entries).expect("expected copy");
    let owned_hash = tree::tree_digest(&tree::collect_tree(&expected).expect("owned tree"))
        .expect("owned digest");
    fs::remove_dir_all(expected).expect("cleanup");
    let info = sha256_hex(b"outer plist");
    let main = sha256_hex(b"outer executable");
    let cli_hash = sha256_hex(b"orbctl bytes");
    let sdk = HostOrbStackSdk {
        app_bundle_path: app.display().to_string(),
        bundle_id: "com.example.orbstack".to_owned(),
        team_id: "TEAM123".to_owned(),
        version: "1".to_owned(),
        build: "1".to_owned(),
        info_plist_sha256: info,
        main_executable_path: "Contents/MacOS/OrbStack".to_owned(),
        main_executable_sha256: main,
        cli_bundle_path: cli.display().to_string(),
        source_tree_sha256: source_hash,
        owned_tree_sha256: owned_hash,
        cli_relative_path: "Contents/MacOS/orbctl".to_owned(),
        cli_sha256: cli_hash,
        cli_version: "1".to_owned(),
        cli_build: "1".to_owned(),
        cli_commit: "a".repeat(40),
        runtime_dir: home.join(".orbstack/run").display().to_string(),
        runtime_uid: 0,
    };
    (temp, home, sdk, cli)
}

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
