use super::snapshot_root;
use crate::qualification_cache_payload::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temporary directory");
    let root = fs::canonicalize(temp.path()).expect("canonical temporary directory");
    (temp, root)
}

fn observe(id: CacheRootId, path: PathBuf) -> CacheRootObservation {
    CacheRootObservation { id, path }
}

#[test]
fn safe_absent_directory_and_file_roots_remain_absent() {
    let (_temp, root) = canonical_tempdir();
    let missing_parent = root.join("missing-parent");
    for (id, path) in [
        (
            CacheRootId::TofuPlugins,
            missing_parent.join("nested-cache-root"),
        ),
        (
            CacheRootId::CargoCrates2Json,
            missing_parent.join(".crates2.json"),
        ),
        (CacheRootId::CargoCratesToml, root.join(".crates.toml")),
    ] {
        assert_eq!(
            snapshot_root(&observe(id, path.clone())),
            Ok(crate::qualification_cache_payload::manifest::PayloadRoot {
                id,
                present: false,
                mode: None,
                digest: None,
            })
        );
    }
}

#[test]
fn missing_directory_and_intermediate_components_reject_unsafe_ancestor() {
    let (_temp, root) = canonical_tempdir();
    let unsafe_parent = root.join("writable-parent");
    fs::create_dir(&unsafe_parent).expect("unsafe ancestor");
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777))
        .expect("writable ancestor");
    let missing_root = unsafe_parent.join("missing-intermediate/nested/cache-root");

    assert_eq!(
        snapshot_root(&observe(CacheRootId::TofuPlugins, missing_root)),
        Err("qualification_cache_payload_source_ancestor_invalid")
    );
}

#[test]
fn missing_file_root_rejects_unsafe_existing_parent() {
    let (_temp, root) = canonical_tempdir();
    let unsafe_parent = root.join("writable-parent");
    fs::create_dir(&unsafe_parent).expect("unsafe ancestor");
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777))
        .expect("writable ancestor");
    let missing_file = unsafe_parent.join("missing-intermediate/.crates2.json");

    assert_eq!(
        snapshot_root(&observe(CacheRootId::CargoCrates2Json, missing_file)),
        Err("qualification_cache_payload_source_ancestor_invalid")
    );
}

#[test]
fn root_set_snapshot_rejects_unsafe_missing_root_before_snapshot_completion() {
    let (_temp, root) = canonical_tempdir();
    let unsafe_parent = root.join("writable-parent");
    fs::create_dir(&unsafe_parent).expect("unsafe ancestor");
    fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777))
        .expect("writable ancestor");
    let roots = CacheRootSet {
        owner_id: "source-absence".to_owned(),
        layer: velnor_actions_contract::workflow::QualificationCacheLayer::MiseTools,
        roots: vec![
            observe(
                CacheRootId::CargoCrates2Json,
                unsafe_parent.join("missing/.crates2.json"),
            ),
            observe(CacheRootId::CargoCratesToml, root.join(".crates.toml")),
        ],
    };

    assert_eq!(
        super::super::snapshot_roots(&roots),
        Err("qualification_cache_payload_source_ancestor_invalid")
    );
}
