use super::{
    read_captured_source_locator,
    snapshot::{Snapshot, snapshot_tree},
};
use std::{fs, path::Path};
use velnor_actions_contract::homebrew_authority::{
    HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH, HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1,
    HOMEBREW_SOURCE_CONFIG_PATH, HOMEBREW_SOURCE_RELEASE_POLICY_PATH,
    HomebrewSourceAuthorityLocator,
};

fn install_locator(root: &Path, locator: &HomebrewSourceAuthorityLocator) -> Snapshot {
    let bytes = locator.canonical_bytes().expect("canonical locator");
    let path = root.join(HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH);
    fs::create_dir_all(path.parent().expect("locator parent")).expect("locator directory");
    fs::write(path, bytes).expect("locator write");
    snapshot_tree(root).expect("captured checkout snapshot")
}

fn locator() -> HomebrewSourceAuthorityLocator {
    HomebrewSourceAuthorityLocator {
        schema: HOMEBREW_SOURCE_AUTHORITY_LOCATOR_SCHEMA_V1.to_owned(),
        repository: "owner/source".to_owned(),
        repository_id: 41,
        default_branch: "main".to_owned(),
        config_path: HOMEBREW_SOURCE_CONFIG_PATH.to_owned(),
        release_policy_path: HOMEBREW_SOURCE_RELEASE_POLICY_PATH.to_owned(),
    }
}

fn locator_tree(root: &Path) -> Vec<velnor_actions_mise::TapTreeEntry> {
    let bytes = fs::read(root.join(HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH)).expect("locator");
    vec![velnor_actions_mise::TapTreeEntry {
        path: HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH.to_owned(),
        mode: 0o100_644,
        blob_sha: crate::release_policy_admission::git_blob_sha1(&bytes).expect("blob SHA"),
    }]
}

#[test]
fn captured_tap_locator_binds_source_identity_and_allows_empty_distribution_policy() {
    let root = tempfile::TempDir::new().expect("temp root");
    fs::create_dir_all(root.path().join(".velnor")).expect("config directory");
    fs::write(root.path().join(".velnor/config.toml"), "[distribution]\n")
        .expect("tap config without source formula policy");
    let locator = locator();
    let snapshot = install_locator(root.path(), &locator);
    let tree = locator_tree(root.path());
    let admitted =
        read_captured_source_locator(root.path(), &tree, &snapshot, "owner/source", 41, "main")
            .expect("canonical locator from captured Git blob");
    assert_eq!(admitted, locator);
}

#[test]
fn captured_tap_locator_rejects_wrong_source_slug_or_id() {
    let root = tempfile::TempDir::new().expect("temp root");
    let snapshot = install_locator(root.path(), &locator());
    let tree = locator_tree(root.path());
    assert!(
        read_captured_source_locator(root.path(), &tree, &snapshot, "owner/other", 41, "main")
            .is_err()
    );
    assert!(
        read_captured_source_locator(root.path(), &tree, &snapshot, "owner/source", 42, "main")
            .is_err()
    );
    assert!(
        read_captured_source_locator(root.path(), &tree, &snapshot, "owner/source", 41, "trunk")
            .is_err()
    );
}

#[test]
fn captured_tap_locator_rejects_tampered_tree_blob_and_checkout_bytes() {
    let root = tempfile::TempDir::new().expect("temp root");
    let snapshot = install_locator(root.path(), &locator());
    let tree = locator_tree(root.path());
    let mut wrong_blob = tree.clone();
    wrong_blob[0].blob_sha = "a".repeat(40);
    assert!(
        read_captured_source_locator(
            root.path(),
            &wrong_blob,
            &snapshot,
            "owner/source",
            41,
            "main"
        )
        .is_err()
    );

    fs::write(
        root.path().join(HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH),
        b"{}",
    )
    .expect("tamper checkout locator");
    assert!(
        read_captured_source_locator(root.path(), &tree, &snapshot, "owner/source", 41, "main")
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn snapshot_refuses_symlinks_anywhere_in_tap_checkout() {
    let root = tempfile::TempDir::new().expect("temp root");
    std::os::unix::fs::symlink("elsewhere", root.path().join("Formula")).expect("symlink");
    assert!(snapshot_tree(root.path()).is_err());
}
