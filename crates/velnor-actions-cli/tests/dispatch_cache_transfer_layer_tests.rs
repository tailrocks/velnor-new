use super::super::{
    CacheRestoreStatus, export_payload, import_payload, payload, root_name, snapshot_root,
    useful_state_digest,
};
use super::{CACHE_VERSION, binding_with_keys};
use crate::dispatch::cache_bind::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation, QualificationCacheLayer,
};

#[test]
fn mbx_cargo_source_and_tofu_layers_roundtrip_isolated_roots() {
    for (layer, name, ids) in [
        (
            QualificationCacheLayer::MbxObjects,
            "mbx_objects",
            vec![CacheRootId::MbxActions],
        ),
        (
            QualificationCacheLayer::CargoSources,
            "cargo_sources",
            vec![
                CacheRootId::CargoGitDb,
                CacheRootId::CargoRegistryCache,
                CacheRootId::CargoRegistryIndex,
            ],
        ),
        (
            QualificationCacheLayer::TofuProviders,
            "tofu_providers",
            vec![CacheRootId::TofuPlugins],
        ),
    ] {
        roundtrip(layer, name, &ids);
    }
}

fn roundtrip(layer: QualificationCacheLayer, name: &str, ids: &[CacheRootId]) {
    let key = format!("velnor-qcache-v3-campaign-1-{name}-runtime-k1");
    let cold = tempfile::tempdir().expect("cold runner temp");
    let cold_temp = runner_temp(&cold);
    let cold_roots = root_set(&cold_temp, layer, ids);
    fill_roots(&cold_roots, "cold");
    let cold_binding = binding_with_keys(name, &key, None, None);
    let exported = export_payload(&cold_binding, &cold_roots, &cold_temp).expect("cold export");
    let stage = exported.save_stage_path.expect("save stage");

    let warm = tempfile::tempdir().expect("warm runner temp");
    let warm_temp = runner_temp(&warm);
    let warm_roots = root_set(&warm_temp, layer, ids);
    fill_roots(&warm_roots, "stale");
    let warm_stage = PathBuf::from(
        crate::dispatch::cache_bind::stage_path(&warm_temp, "campaign-1", name, &key)
            .expect("warm restore path"),
    );
    fs::create_dir_all(warm_stage.parent().expect("stage parent")).expect("stage parents");
    payload::copy_tree_to_new_dir(&stage, &warm_stage).expect("restore staged payload");
    let entry = cache_entry(&key);
    let warm_binding = binding_with_keys(
        name,
        &key,
        Some(entry.clone()),
        Some(exported.state_digest.clone()),
    );
    let proof = import_payload(
        &warm_binding,
        &warm_roots,
        &warm_temp,
        true,
        Some(&key),
        &QualificationCacheBackendObservation::Found(entry),
    )
    .expect("warm import");
    assert_eq!(proof.status, CacheRestoreStatus::Hit);
    let restored_state = useful_state_digest(
        &warm_roots
            .roots
            .iter()
            .map(snapshot_root)
            .collect::<Result<Vec<_>, _>>()
            .expect("post-import inventory"),
    )
    .expect("post-import digest");
    assert_eq!(restored_state, exported.state_digest, "layer {name}");
}

fn root_set(
    runner_temp: &Path,
    layer: QualificationCacheLayer,
    ids: &[CacheRootId],
) -> CacheRootSet {
    CacheRootSet {
        matrix_key: "lane-1".to_owned(),
        layer,
        roots: ids
            .iter()
            .map(|id| CacheRootObservation {
                id: *id,
                path: root_path(runner_temp, *id),
            })
            .collect(),
    }
}

fn root_path(runner_temp: &Path, id: CacheRootId) -> PathBuf {
    match id {
        CacheRootId::MbxActions => runner_temp.join("velnor/mbx/actions"),
        CacheRootId::CargoGitDb => runner_temp.join("velnor/cargo/git/db"),
        CacheRootId::CargoRegistryCache => runner_temp.join("velnor/cargo/registry/cache"),
        CacheRootId::CargoRegistryIndex => runner_temp.join("velnor/cargo/registry/index"),
        CacheRootId::TofuPlugins => runner_temp.join("velnor/tofu-cache/plugins"),
        _ => panic!("unsupported test root {}", root_name(id)),
    }
}

fn fill_roots(roots: &CacheRootSet, state: &str) {
    for root in &roots.roots {
        let marker = root.path.join("cache-marker");
        fs::create_dir_all(root.path.parent().expect("root parent")).expect("root parent dirs");
        fs::create_dir_all(&root.path).expect("cache root");
        fs::write(marker, format!("{state}-{}\n", root_name(root.id))).expect("marker");
    }
}

fn runner_temp(temp: &tempfile::TempDir) -> PathBuf {
    let path = temp.path().join("runner-temp");
    fs::create_dir_all(&path).expect("runner temp directory");
    fs::canonicalize(path).expect("canonical runner temp path")
}

fn cache_entry(key: &str) -> QualificationCacheBackendEntry {
    QualificationCacheBackendEntry {
        id: 71,
        key: key.to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        version: CACHE_VERSION.to_owned(),
        size_bytes: 512,
    }
}
