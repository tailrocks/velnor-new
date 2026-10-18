use crate::qualification_cache_payload::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use crate::qualification_cache_runtime::QualificationCacheBindingOutputs;
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_contract::workflow::{QualificationCacheBackendEntry, QualificationCacheLayer};

pub(super) fn exported_fixture(
    name: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    CacheRootSet,
    PathBuf,
    super::super::CacheExportProof,
) {
    let temp = tempfile::tempdir().expect("runner temp");
    let runner_temp = runner_temp(&temp, name);
    let roots = roots(&runner_temp);
    create_live_roots(&roots, "cold-state");
    let proof = super::export_payload(&binding(None, None), &roots, &runner_temp).expect("export");
    create_live_roots(&roots, "stale-state");
    let stage = proof.save_stage_path.clone().expect("save stage");
    (temp, stage, roots, runner_temp, proof)
}

pub(super) fn runner_temp(temp: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = temp.path().join(name);
    fs::create_dir_all(&path).expect("runner temp directory");
    fs::canonicalize(path).expect("canonical runner temp path")
}

pub(super) fn roots(runner_temp: &Path) -> CacheRootSet {
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

pub(super) fn create_live_roots(roots: &CacheRootSet, state: &str) {
    for root in &roots.roots {
        if matches!(
            root.id,
            CacheRootId::CargoCrates2Json | CacheRootId::CargoCratesToml
        ) {
            fs::create_dir_all(root.path.parent().expect("cargo home")).expect("cargo parent");
            fs::write(
                &root.path,
                format!("{state}-{}", super::super::root_name(root.id)),
            )
            .expect("cargo marker");
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

pub(super) fn binding(
    expected_cache: Option<QualificationCacheBackendEntry>,
    expected_prior_state_digest: Option<String>,
) -> QualificationCacheBindingOutputs {
    binding_with_keys(
        "mise_tools",
        super::CACHE_KEY,
        expected_cache,
        expected_prior_state_digest,
    )
}

pub(super) fn binding_for_layer(layer: &str, cache_key: &str) -> QualificationCacheBindingOutputs {
    binding_with_keys(layer, cache_key, None, None)
}

pub(super) fn binding_with_keys(
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

pub(super) fn cache_entry(runner_temp: &Path) -> QualificationCacheBackendEntry {
    let key = super::CACHE_KEY;
    let stage_path = crate::qualification_cache_payload::qualification_cache_stage_path(
        runner_temp,
        "campaign-1",
        "mise_tools",
        key,
    )
    .expect("checked action stage path");
    let version =
        crate::qualification_cache_payload::cache_action_version::version_for_action_path(
            &stage_path,
            crate::qualification_cache_payload::cache_action_version::CompressionMethod::Gzip,
        )
        .expect("pinned action version");
    QualificationCacheBackendEntry {
        id: 71,
        key: key.to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        version,
        size_bytes: 512,
    }
}
