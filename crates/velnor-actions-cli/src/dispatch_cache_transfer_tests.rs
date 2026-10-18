use super::{
    CacheRestoreStatus, export_payload, import_payload, payload, root_name, useful_state_digest,
};
use crate::dispatch::cache_bind::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation, QualificationCacheLayer,
};
use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

const CACHE_KEY: &str = "velnor-qcache-v2-campaign-1-mise_tools-runtime-k1";
const CACHE_VERSION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[path = "dispatch_cache_transfer_layer_tests.rs"]
mod layer_tests;

#[test]
fn cold_export_and_warm_import_restore_exact_file_and_directory_roots() {
    let cold = tempfile::tempdir().expect("cold runner temp");
    let cold_temp = runner_temp(&cold, "runner-temp");
    let cold_roots = roots(&cold_temp);
    create_live_roots(&cold_roots, "cold-state");
    let cold_binding = binding(None, None);
    let exported = export_payload(&cold_binding, &cold_roots, &cold_temp).expect("cold export");
    assert_eq!(exported.save_key.as_deref(), Some(CACHE_KEY));
    let stage = exported.save_stage_path.as_ref().expect("save stage");
    assert!(stage.join("manifest.json").is_file());

    let warm = tempfile::tempdir().expect("warm runner temp");
    let warm_temp = runner_temp(&warm, "runner-temp");
    let warm_roots = roots(&warm_temp);
    create_live_roots(&warm_roots, "stale-state");
    let warm_stage = PathBuf::from(
        crate::dispatch::cache_bind::stage_path(&warm_temp, "campaign-1", "mise_tools", CACHE_KEY)
            .expect("warm restore path"),
    );
    fs::create_dir_all(warm_stage.parent().expect("stage parent")).expect("stage parents");
    payload::copy_tree_to_new_dir(stage, &warm_stage).expect("simulate cache restore");

    let expected = cache_entry();
    let warm_binding = binding(Some(expected.clone()), Some(exported.state_digest.clone()));
    let imported = import_payload(
        &warm_binding,
        &warm_roots,
        &warm_temp,
        true,
        Some(CACHE_KEY),
        &QualificationCacheBackendObservation::Found(expected),
    )
    .expect("warm import");
    assert_eq!(imported.status, CacheRestoreStatus::Hit);
    assert_eq!(
        imported.state_digest.as_deref(),
        Some(exported.state_digest.as_str())
    );
    assert_eq!(
        fs::read(warm_roots.roots[1].path.as_path()).expect("restored cargo metadata"),
        b"cold-state-cargo_crates2_json"
    );
    assert_eq!(
        fs::read_to_string(warm_roots.roots[3].path.join("installs/rust/current"))
            .expect("restored mise tool"),
        "cold-state-rust\n"
    );
    assert!(!warm_roots.roots[0].path.join("stale").exists());
    let actual = useful_state_digest(
        &warm_roots
            .roots
            .iter()
            .map(super::snapshot_root)
            .collect::<Result<Vec<_>, _>>()
            .expect("post-import inventory"),
    )
    .expect("post-import digest");
    assert_eq!(actual, exported.state_digest);
}

#[test]
fn backend_mismatch_and_changed_payload_never_modify_live_roots() {
    let (_temp, stage, roots, runner_temp, exported) = exported_fixture("backend-mismatch");
    let binding = binding(Some(cache_entry()), Some(exported.state_digest.clone()));
    let before = fs::read(&roots.roots[1].path).expect("old file root");
    assert_eq!(
        import_payload(
            &binding,
            &roots,
            &runner_temp,
            true,
            Some(CACHE_KEY),
            &QualificationCacheBackendObservation::Absent,
        ),
        Err("qualification_cache_restore_backend_mismatch")
    );
    assert_eq!(
        fs::read(&roots.roots[1].path).expect("old file root"),
        before
    );

    fs::write(
        stage.join("roots/mise_data/installs/rust/current"),
        "tampered\n",
    )
    .expect("tamper staged payload");
    assert!(
        import_payload(
            &binding,
            &roots,
            &runner_temp,
            true,
            Some(CACHE_KEY),
            &QualificationCacheBackendObservation::Found(cache_entry()),
        )
        .is_err()
    );
    assert_eq!(
        fs::read(&roots.roots[1].path).expect("old file root"),
        before
    );
}

#[test]
fn archive_layout_and_exact_restore_predecessor_are_required() {
    let (_temp, stage, roots, runner_temp, exported) = exported_fixture("layout-check");
    let binding = binding(Some(cache_entry()), Some(exported.state_digest.clone()));
    fs::write(stage.join("unchecked.bin"), b"extra archive entry").expect("add unknown entry");
    assert_eq!(
        import_payload(
            &binding,
            &roots,
            &runner_temp,
            true,
            Some(CACHE_KEY),
            &QualificationCacheBackendObservation::Found(cache_entry()),
        ),
        Err("qualification_cache_payload_layout_invalid")
    );
    assert_eq!(
        fs::read(&roots.roots[1].path).expect("old file root"),
        b"stale-state-cargo_crates2_json"
    );
}

#[test]
fn predecessor_digest_and_full_backend_identity_must_match_before_import() {
    let (_temp, stage, roots, runner_temp, exported) = exported_fixture("identity-check");
    let wrong_predecessor = format!("b3-{}", "d".repeat(64));
    let binding = binding(Some(cache_entry()), Some(wrong_predecessor));
    let before = fs::read(&roots.roots[1].path).expect("old file root");
    assert_eq!(
        import_payload(
            &binding,
            &roots,
            &runner_temp,
            true,
            Some(CACHE_KEY),
            &QualificationCacheBackendObservation::Found(cache_entry()),
        ),
        Err("qualification_cache_manifest_binding_mismatch")
    );
    assert_eq!(
        fs::read(&roots.roots[1].path).expect("old file root"),
        before
    );
    assert!(stage.join("manifest.json").is_file());
    assert_ne!(exported.state_digest, "");
}

#[test]
fn wrong_backend_object_fields_or_action_hit_never_import() {
    let (_temp, _stage, roots, runner_temp, exported) = exported_fixture("backend-fields");
    let binding = binding(Some(cache_entry()), Some(exported.state_digest));
    let before = fs::read(&roots.roots[1].path).expect("old file root");
    let expected = cache_entry();
    let mismatches = vec![
        QualificationCacheBackendEntry {
            id: 72,
            ..expected.clone()
        },
        QualificationCacheBackendEntry {
            key: "other-key".to_owned(),
            ..expected.clone()
        },
        QualificationCacheBackendEntry {
            git_ref: "refs/heads/other".to_owned(),
            ..expected.clone()
        },
        QualificationCacheBackendEntry {
            version: "e".repeat(64),
            ..expected.clone()
        },
        QualificationCacheBackendEntry {
            size_bytes: expected.size_bytes + 1,
            ..expected.clone()
        },
    ];
    for mismatch in mismatches {
        assert_eq!(
            import_payload(
                &binding,
                &roots,
                &runner_temp,
                true,
                Some(CACHE_KEY),
                &QualificationCacheBackendObservation::Found(mismatch),
            ),
            Err("qualification_cache_restore_backend_mismatch")
        );
    }
    for (hit, matched_key) in [(false, Some(CACHE_KEY)), (true, Some("wrong-prefix"))] {
        assert_eq!(
            import_payload(
                &binding,
                &roots,
                &runner_temp,
                hit,
                matched_key,
                &QualificationCacheBackendObservation::Found(expected.clone()),
            ),
            Err("qualification_cache_restore_backend_mismatch")
        );
    }
    assert_eq!(
        fs::read(&roots.roots[1].path).expect("old file root"),
        before
    );
}

#[test]
fn a_verified_cold_miss_has_no_payload_import() {
    let temp = tempfile::tempdir().expect("runner temp");
    let runner_temp = runner_temp(&temp, "runner-temp");
    let roots = roots(&runner_temp);
    create_live_roots(&roots, "cold-state");
    let proof = import_payload(
        &binding(None, None),
        &roots,
        &runner_temp,
        false,
        None,
        &QualificationCacheBackendObservation::Absent,
    )
    .expect("cold miss");
    assert_eq!(proof.status, CacheRestoreStatus::Miss);
    assert_eq!(proof.state_digest, None);
}

#[cfg(unix)]
#[test]
fn source_root_symlink_escape_is_rejected_before_stage_creation() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("runner temp");
    let runner_temp = runner_temp(&temp, "runner-temp");
    let roots = CacheRootSet {
        matrix_key: "lane-1".to_owned(),
        layer: QualificationCacheLayer::MbxObjects,
        roots: vec![CacheRootObservation {
            id: CacheRootId::MbxActions,
            path: runner_temp.join("velnor/mbx/actions"),
        }],
    };
    fs::create_dir_all(&roots.roots[0].path).expect("mbx actions root");
    symlink("../../outside", roots.roots[0].path.join("escape")).expect("escaping link");
    let binding = binding_for_layer("mbx_objects", "velnor-qcache-v2-campaign-1-mbx_objects-k1");
    assert!(export_payload(&binding, &roots, &runner_temp).is_err());
    assert!(!runner_temp.join("velnor/qualification-cache-v2").exists());
}

fn exported_fixture(
    name: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    CacheRootSet,
    PathBuf,
    super::CacheExportProof,
) {
    let temp = tempfile::tempdir().expect("runner temp");
    let runner_temp = runner_temp(&temp, name);
    let roots = roots(&runner_temp);
    create_live_roots(&roots, "cold-state");
    let proof = export_payload(&binding(None, None), &roots, &runner_temp).expect("export");
    create_live_roots(&roots, "stale-state");
    let stage = proof.save_stage_path.clone().expect("save stage");
    (temp, stage, roots, runner_temp, proof)
}

fn runner_temp(temp: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = temp.path().join(name);
    fs::create_dir_all(&path).expect("runner temp directory");
    fs::canonicalize(path).expect("canonical runner temp path")
}

fn roots(runner_temp: &Path) -> CacheRootSet {
    let cargo = runner_temp.join("velnor/cargo");
    CacheRootSet {
        matrix_key: "lane-1".to_owned(),
        layer: QualificationCacheLayer::MiseTools,
        roots: vec![
            CacheRootObservation {
                id: CacheRootId::CargoBin,
                path: cargo.join("bin"),
            },
            CacheRootObservation {
                id: CacheRootId::CargoCrates2Json,
                path: cargo.join(".crates2.json"),
            },
            CacheRootObservation {
                id: CacheRootId::CargoCratesToml,
                path: cargo.join(".crates.toml"),
            },
            CacheRootObservation {
                id: CacheRootId::MiseData,
                path: runner_temp.join("velnor/mise"),
            },
            CacheRootObservation {
                id: CacheRootId::RustupHome,
                path: runner_temp.join("velnor/rustup"),
            },
        ],
    }
}

fn create_live_roots(roots: &CacheRootSet, state: &str) {
    for root in &roots.roots {
        if matches!(
            root.id,
            CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml
        ) {
            fs::create_dir_all(root.path.parent().expect("cargo home")).expect("cargo parent");
            fs::write(&root.path, format!("{state}-{}", root_name(root.id))).expect("cargo marker");
        } else {
            fs::create_dir_all(root.path.join("installs/rust")).expect("cache directory root");
            if state == "stale-state" {
                fs::write(root.path.join("stale"), b"old").expect("stale marker");
            }
            if root.id == CacheRootId::MiseData {
                fs::write(
                    root.path.join("installs/rust/current"),
                    format!("{state}-rust\n"),
                )
                .expect("tool payload");
            }
        }
    }
}

fn binding(
    expected_cache: Option<QualificationCacheBackendEntry>,
    expected_prior_state_digest: Option<String>,
) -> QualificationCacheBindingOutputs {
    binding_with_keys(
        "mise_tools",
        CACHE_KEY,
        expected_cache,
        expected_prior_state_digest,
    )
}

fn binding_for_layer(layer: &str, cache_key: &str) -> QualificationCacheBindingOutputs {
    binding_with_keys(layer, cache_key, None, None)
}

fn binding_with_keys(
    layer: &str,
    cache_key: &str,
    expected_cache: Option<QualificationCacheBackendEntry>,
    expected_prior_state_digest: Option<String>,
) -> QualificationCacheBindingOutputs {
    QualificationCacheBindingOutputs {
        active: true,
        campaign: "campaign-1".to_owned(),
        phase: "cold".to_owned(),
        plan_id: "plan-1".to_owned(),
        run_key: "r10-a1".to_owned(),
        matrix_key: "lane-1".to_owned(),
        layer: layer.to_owned(),
        restore_key: Some(cache_key.to_owned()),
        expected_cache,
        save_key: Some(cache_key.to_owned()),
        save_if_state_changes: false,
        expected_prior_state_digest,
        runtime_identity_digest: Some(format!("b3-{}", "c".repeat(64))),
    }
}

fn cache_entry() -> QualificationCacheBackendEntry {
    QualificationCacheBackendEntry {
        id: 71,
        key: CACHE_KEY.to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        version: CACHE_VERSION.to_owned(),
        size_bytes: 512,
    }
}
